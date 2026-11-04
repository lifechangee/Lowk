//! Q&A link tracking. An answer starts on Ask, collects Me speech, and closes
//! on the next Ask, explicit Done, or call end. Its duration limit is a cap,
//! not an automatic close timer.
use lowk_types::{CloseReason, MonoNs, MyAnswerState, QaId, QaRecord, QaUpdate, Stream, SttEvent, SttKind};

const ANSWER_LIMIT_NS: u64 = 180_000_000_000;
const ANSWER_LIMIT_CHARS: usize = 2_000;

/// Returned for the session to journal (`CallEntry::MyAnswer`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosedAnswer {
    pub qa_id: QaId,
    pub text: String,
    pub first_at: Option<MonoNs>,
    pub reason: CloseReason,
}

#[derive(Default, Debug)]
pub struct QaOutput {
    pub updates: Vec<QaUpdate>,
    pub closed: Vec<ClosedAnswer>,
}

pub struct QaTracker {
    records: Vec<QaRecord>,
    /// Index into `records` of the current answer record, including a capped answer awaiting close.
    open: Option<usize>,
    partial: String,
    mic_on: bool,
    started_at: MonoNs,
    /// Utterance ids let background corrections replace the corresponding Me final.
    segments: Vec<(usize, u64, String)>,
    last_partial_emit: MonoNs,
    limited: bool,
}

impl Default for QaTracker {
    fn default() -> Self {
        Self {
            records: vec![],
            open: None,
            partial: String::new(),
            mic_on: true,
            started_at: 0,
            segments: vec![],
            last_partial_emit: 0,
            limited: false,
        }
    }
}

impl QaTracker {
    pub fn initial_state(&mut self, at: MonoNs) -> QaUpdate {
        self.started_at = at;
        self.update(None, MyAnswerState::BeforeFirstAsk, at)
    }

    pub fn records(&self) -> &[QaRecord] {
        &self.records
    }

    pub fn open_qa(&self) -> Option<QaId> {
        self.open.map(|i| self.records[i].qa_id)
    }

    pub fn get(&self, qa: &QaId) -> Option<&QaRecord> {
        self.records.iter().find(|r| &r.qa_id == qa)
    }

    /// «Мой ответ» text for `qa` (also valid after the link closed).
    pub fn my_answer(&self, qa: &QaId) -> Option<&str> {
        self.get(qa).map(|r| r.my_answer.as_str())
    }

    pub fn current_answer(&self) -> Option<&str> {
        self.open.map(|i| self.records[i].my_answer.as_str())
    }

    fn update(&self, i: Option<usize>, state: MyAnswerState, at: MonoNs) -> QaUpdate {
        let r = i.map(|i| &self.records[i]);
        let text = r.map_or_else(String::new, |r| r.my_answer.clone());
        QaUpdate {
            qa_id: r.map(|r| r.qa_id),
            state,
            char_count: text.chars().count() as u32,
            text,
            partial_text: self.partial.clone(),
            elapsed_ms: at.saturating_sub(self.started_at) / 1_000_000,
        }
    }

    /// Arm the record at the instant Ask is pressed; collection overlaps hint generation.
    pub fn arm(&mut self, qa_id: QaId, question: String, hint: String, armed_at: MonoNs) -> QaOutput {
        let mut out = self.close_at(CloseReason::NextAsk, armed_at);
        self.records.push(QaRecord {
            qa_id,
            question,
            hint,
            my_answer: String::new(),
            armed_at,
            first_at: None,
            closed: None,
        });
        self.open = Some(self.records.len() - 1);
        self.started_at = armed_at;
        self.partial.clear();
        self.limited = false;
        self.last_partial_emit = 0;
        out.updates.push(self.update(
            self.open,
            if self.mic_on {
                MyAnswerState::Listening
            } else {
                MyAnswerState::Paused
            },
            armed_at,
        ));
        out
    }

    /// The hint of an already armed link finished streaming.
    pub fn set_hint(&mut self, qa: &QaId, hint: String) {
        if let Some(r) = self.records.iter_mut().find(|r| &r.qa_id == qa) {
            r.hint = hint;
        }
    }

    /// The Ask failed: forget the link so a hint-less record never reaches the journal.
    pub fn discard(&mut self, qa: &QaId) {
        if let Some(i) = self.records.iter().position(|r| &r.qa_id == qa) {
            if self.records[i].my_answer.is_empty() {
                self.records.remove(i);
                self.open = match self.open {
                    Some(o) if o == i => None,
                    Some(o) if o > i => Some(o - 1),
                    other => other,
                };
            }
        }
    }

    pub fn on_stt(&mut self, ev: &SttEvent) -> QaOutput {
        let mut out = QaOutput::default();
        let Some(i) = self.open else { return out };
        if self.limited {
            return out;
        }
        // Only the user's own finals count; System speech never touches the link.
        if ev.stream != Stream::Me || !self.mic_on {
            return out;
        }
        let text = ev.text.trim();
        // Speech that ended before the hint was armed is not an answer to it.
        if text.is_empty() || ev.t_audio_end_ns <= self.records[i].armed_at {
            return out;
        }
        if ev.t_audio_end_ns.saturating_sub(self.started_at) >= ANSWER_LIMIT_NS {
            self.limited = true;
            out.updates
                .push(self.update(Some(i), MyAnswerState::Limit, ev.t_audio_end_ns));
            return out;
        }
        if ev.kind == SttKind::Partial {
            self.partial = text.to_owned();
            if ev.t_audio_end_ns.saturating_sub(self.last_partial_emit) >= 150_000_000 {
                self.last_partial_emit = ev.t_audio_end_ns;
                out.updates
                    .push(self.update(Some(i), MyAnswerState::Listening, ev.t_audio_end_ns));
            }
            return out;
        }
        self.partial.clear();
        let existing = self.records[i].my_answer.chars().count();
        let available = ANSWER_LIMIT_CHARS.saturating_sub(existing);
        let separator = existing > 0 && available > 1;
        let room = available.saturating_sub(usize::from(separator));
        if room == 0 {
            self.limited = true;
            out.updates
                .push(self.update(Some(i), MyAnswerState::Limit, ev.t_audio_end_ns));
            return out;
        }
        let final_text: String = text.chars().take(room).collect();
        let r = &mut self.records[i];
        if separator {
            r.my_answer.push(' ');
        }
        r.my_answer.push_str(&final_text);
        self.segments.push((i, ev.utt, final_text));
        r.first_at.get_or_insert(ev.t_start_ns);
        let limit = r.my_answer.chars().count() >= ANSWER_LIMIT_CHARS;
        if limit {
            self.limited = true;
        }
        out.updates.push(self.update(
            Some(i),
            if limit {
                MyAnswerState::Limit
            } else {
                MyAnswerState::Listening
            },
            ev.t_audio_end_ns,
        ));
        out
    }

    pub fn set_mic(&mut self, on: bool, at: MonoNs) -> QaOutput {
        self.mic_on = on;
        let Some(i) = self.open else {
            return QaOutput::default();
        };
        if self.limited {
            return QaOutput::default();
        }
        self.partial.clear();
        QaOutput {
            updates: vec![self.update(
                Some(i),
                if on {
                    MyAnswerState::Listening
                } else {
                    MyAnswerState::Paused
                },
                at,
            )],
            closed: vec![],
        }
    }

    pub fn correct(&mut self, ev: &SttEvent) -> QaOutput {
        let Some(pos) = self.segments.iter().position(|(_, utt, _)| *utt == ev.utt) else {
            return QaOutput::default();
        };
        let i = self.segments[pos].0;
        self.segments[pos].2 = ev.text.trim().to_owned();
        let joined = self
            .segments
            .iter()
            .filter(|(record, _, _)| *record == i)
            .map(|(_, _, s)| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        self.records[i].my_answer = joined.chars().take(ANSWER_LIMIT_CHARS).collect();
        let state = if self.limited && self.open == Some(i) {
            MyAnswerState::Limit
        } else if self.open == Some(i) {
            MyAnswerState::Listening
        } else {
            MyAnswerState::Closed
        };
        QaOutput {
            updates: vec![self.update(Some(i), state, ev.t_emit_ns)],
            closed: vec![],
        }
    }

    pub fn close(&mut self, reason: CloseReason) -> QaOutput {
        self.close_at(reason, lowk_types::now_mono_ns())
    }

    fn close_at(&mut self, reason: CloseReason, at: MonoNs) -> QaOutput {
        let mut out = QaOutput::default();
        if let Some(i) = self.open.take() {
            self.records[i].closed = Some(reason);
            self.partial.clear();
            out.updates.push(self.update(Some(i), MyAnswerState::Closed, at));
            let r = &self.records[i];
            if !r.my_answer.is_empty() {
                out.closed.push(ClosedAnswer {
                    qa_id: r.qa_id,
                    text: r.my_answer.clone(),
                    first_at: r.first_at,
                    reason,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::Ulid;

    const S: u64 = 1_000_000_000;

    fn fin(stream: Stream, start_s: u64, end_s: u64, text: &str) -> SttEvent {
        SttEvent {
            stream,
            utt: start_s,
            rev: 0,
            kind: SttKind::Final,
            text: text.into(),
            lang: None,
            t_start_ns: start_s * S,
            t_audio_end_ns: end_s * S,
            t_emit_ns: end_s * S,
        }
    }
    fn qa(n: u128) -> QaId {
        Ulid::from_parts(0, n)
    }

    #[test]
    fn collects_me_finals_and_ignores_system_speech() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "hint".into(), 10 * S);
        // Me speech before arming is ignored.
        t.on_stt(&fin(Stream::Me, 5, 9, "старое"));
        let o = t.on_stt(&fin(Stream::System, 11, 12, "угу"));
        assert!(o.closed.is_empty() && t.open_qa().is_some());
        let o1 = t.on_stt(&fin(Stream::Me, 12, 15, "Это прибыль"));
        assert_eq!(o1.updates[0].text, "Это прибыль");
        let o2 = t.on_stt(&fin(Stream::Me, 16, 18, "до налогов."));
        assert_eq!(o2.updates[0].text, "Это прибыль до налогов.");
        // F14: a long interviewer utterance does NOT close the link...
        let o3 = t.on_stt(&fin(Stream::System, 22, 40, "Теперь следующий вопрос."));
        assert!(o3.closed.is_empty() && o3.updates.is_empty());
        assert_eq!(t.open_qa(), Some(qa(1)));
        // ...and the user's later speech keeps accumulating.
        let o4 = t.on_stt(&fin(Stream::Me, 41, 42, "и ещё."));
        assert_eq!(o4.updates[0].text, "Это прибыль до налогов. и ещё.");
    }

    #[test]
    fn time_limit_is_a_state_not_a_timer_close() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        t.on_stt(&fin(Stream::Me, 1, 2, "раз"));
        // At the three minute cap the record stops accepting speech.
        let o = t.on_stt(&fin(Stream::Me, 7200, 7201, "два"));
        assert_eq!(o.updates[0].state, MyAnswerState::Limit);
        assert_eq!(o.updates[0].text, "раз");
        assert_eq!(t.open_qa(), Some(qa(1)));
    }

    #[test]
    fn next_ask_and_rearm_close_previous_link() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q1".into(), "h1".into(), 0);
        t.on_stt(&fin(Stream::Me, 1, 2, "ответ один"));
        let o = t.close(CloseReason::NextAsk);
        assert_eq!(o.closed[0].reason, CloseReason::NextAsk);
        // Arming while another is open closes it too.
        t.arm(qa(2), "q2".into(), "h2".into(), 10 * S);
        t.on_stt(&fin(Stream::Me, 11, 12, "ответ два"));
        let o = t.arm(qa(3), "q3".into(), "h3".into(), 20 * S);
        assert_eq!(o.closed[0].qa_id, qa(2));
        assert_eq!(t.my_answer(&qa(2)), Some("ответ два"));
        assert_eq!(t.records().len(), 3);
        assert_eq!(t.open_qa(), Some(qa(3)));
    }

    #[test]
    fn empty_answer_closes_without_journal_entry() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        let o = t.close(CloseReason::SessionEnd);
        assert!(o.closed.is_empty());
        assert_eq!(o.updates[0].state, MyAnswerState::Closed);
    }

    #[test]
    fn partials_are_shown_and_debounced() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        let mut p = fin(Stream::Me, 1, 2, "ну");
        p.kind = SttKind::Partial;
        assert_eq!(t.on_stt(&p).updates[0].partial_text, "ну");
        p.text = "ну да".into();
        p.t_audio_end_ns += 50_000_000;
        assert!(t.on_stt(&p).updates.is_empty());
        p.t_audio_end_ns += 150_000_000;
        assert_eq!(t.on_stt(&p).updates[0].partial_text, "ну да");
    }

    #[test]
    fn qa_done_is_an_explicit_close_reason() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        t.on_stt(&fin(Stream::Me, 1, 2, "готово"));
        let out = t.close(CloseReason::QaDone);
        assert_eq!(out.closed[0].reason, CloseReason::QaDone);
        assert_eq!(out.updates[0].state, MyAnswerState::Closed);
    }

    #[test]
    fn mic_pause_and_resume_keeps_one_answer() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        assert_eq!(t.set_mic(false, S).updates[0].state, MyAnswerState::Paused);
        assert!(t.on_stt(&fin(Stream::Me, 2, 3, "ignored")).updates.is_empty());
        assert_eq!(t.set_mic(true, 4 * S).updates[0].state, MyAnswerState::Listening);
        t.on_stt(&fin(Stream::Me, 5, 6, "сказал"));
        assert_eq!(t.my_answer(&qa(1)), Some("сказал"));
    }

    #[test]
    fn char_limit_truncates_and_locks_answer() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        let o = t.on_stt(&fin(Stream::Me, 1, 2, &"я".repeat(2_001)));
        assert_eq!(o.updates[0].state, MyAnswerState::Limit);
        assert_eq!(o.updates[0].char_count, 2_000);
        assert!(t.on_stt(&fin(Stream::Me, 3, 4, "лишнее")).updates.is_empty());
    }

    #[test]
    fn background_correction_replaces_answer_segment() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        t.on_stt(&fin(Stream::Me, 1, 2, "черновик"));
        let mut corrected = fin(Stream::Me, 1, 2, "исправлено");
        corrected.utt = 1;
        let o = t.correct(&corrected);
        assert_eq!(o.updates[0].text, "исправлено");
    }
}
