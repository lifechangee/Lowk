//! Q&A link tracking (ARCHITECTURE 5.5).
//!
//! question -> AI hint -> «Мой ответ». When a hint finishes, a [`QaTracker`]
//! arms one link. It collects the `Final` segments of the Me stream until the
//! first of: the next Ask, a System final of >= 3 s that follows at least one
//! Me segment (the interviewer moved on), 90 s after arming, session end.
//! All times are event time (`MonoNs` carried by the events), so behaviour is
//! deterministic and replayable.
use lowk_types::{MonoNs, QaId, Stream, SttEvent, SttKind};
use serde::{Deserialize, Serialize};

pub const MY_ANSWER_TIMEOUT_NS: u64 = 90_000_000_000;
pub const LONG_SYSTEM_NS: u64 = 3_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    NextAsk,
    SystemQuestion,
    Timeout,
    SessionEnd,
}

/// One question/hint/answer triple.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QaRecord {
    pub qa_id: QaId,
    pub question: String,
    pub hint: String,
    pub my_answer: String,
    pub armed_at: MonoNs,
    /// Event time of the first collected Me segment.
    pub first_at: Option<MonoNs>,
    /// `None` while the link is still collecting.
    pub closed: Option<CloseReason>,
}

/// Emitted whenever the «Мой ответ» text of a link changes or the link closes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QaUpdate {
    pub qa_id: QaId,
    pub text: String,
    pub closed: Option<CloseReason>,
}

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

#[derive(Default)]
pub struct QaTracker {
    records: Vec<QaRecord>,
    /// Index into `records` of the link that is still collecting.
    open: Option<usize>,
}

impl QaTracker {
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

    /// Text of the most recent link with any content (for the `complement`
    /// template: everything the user said since the last AI answer).
    pub fn latest_my_answer(&self) -> Option<&str> {
        self.records
            .iter()
            .rev()
            .find(|r| !r.my_answer.is_empty())
            .map(|r| r.my_answer.as_str())
    }

    /// The next Ask starts: close the collecting link.
    pub fn on_ask(&mut self) -> QaOutput {
        self.close(CloseReason::NextAsk)
    }

    /// A hint finished streaming: arm a new link (closing any older one).
    pub fn arm(&mut self, qa_id: QaId, question: String, hint: String, armed_at: MonoNs) -> QaOutput {
        let out = self.close(CloseReason::NextAsk);
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
        out
    }

    /// Time passed without an event (watchdog / virtual clock).
    pub fn tick(&mut self, now: MonoNs) -> QaOutput {
        match self.open {
            Some(i) if now > self.records[i].armed_at + MY_ANSWER_TIMEOUT_NS => {
                self.close(CloseReason::Timeout)
            }
            _ => QaOutput::default(),
        }
    }

    pub fn on_stt(&mut self, ev: &SttEvent) -> QaOutput {
        let mut out = self.tick(ev.t_audio_end_ns);
        let Some(i) = self.open else { return out };
        if ev.kind != SttKind::Final {
            return out;
        }
        match ev.stream {
            Stream::System => {
                let long = ev.t_audio_end_ns.saturating_sub(ev.t_start_ns) >= LONG_SYSTEM_NS;
                if long && !self.records[i].my_answer.is_empty() {
                    let c = self.close(CloseReason::SystemQuestion);
                    out.updates.extend(c.updates);
                    out.closed.extend(c.closed);
                }
            }
            Stream::Me => {
                let text = ev.text.trim();
                // Speech that ended before the hint was armed is not an answer to it.
                if text.is_empty() || ev.t_audio_end_ns <= self.records[i].armed_at {
                    return out;
                }
                let r = &mut self.records[i];
                if !r.my_answer.is_empty() {
                    r.my_answer.push(' ');
                }
                r.my_answer.push_str(text);
                r.first_at.get_or_insert(ev.t_start_ns);
                out.updates.push(QaUpdate {
                    qa_id: r.qa_id,
                    text: r.my_answer.clone(),
                    closed: None,
                });
            }
        }
        out
    }

    pub fn close(&mut self, reason: CloseReason) -> QaOutput {
        let mut out = QaOutput::default();
        if let Some(i) = self.open.take() {
            let r = &mut self.records[i];
            r.closed = Some(reason);
            out.updates.push(QaUpdate {
                qa_id: r.qa_id,
                text: r.my_answer.clone(),
                closed: Some(reason),
            });
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
    fn collects_me_finals_until_long_system_utterance() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "hint".into(), 10 * S);
        // Me speech before arming is ignored.
        t.on_stt(&fin(Stream::Me, 5, 9, "старое"));
        // A short System remark with no Me answer yet does not close the link.
        let o = t.on_stt(&fin(Stream::System, 11, 12, "угу"));
        assert!(o.closed.is_empty() && t.open_qa().is_some());
        let o1 = t.on_stt(&fin(Stream::Me, 12, 15, "Это прибыль"));
        assert_eq!(o1.updates[0].text, "Это прибыль");
        let o2 = t.on_stt(&fin(Stream::Me, 16, 18, "до налогов."));
        assert_eq!(o2.updates[0].text, "Это прибыль до налогов.");
        // A short System utterance after Me does not close it either...
        assert!(t.on_stt(&fin(Stream::System, 19, 21, "ага")).closed.is_empty());
        // ...a >= 3 s one does.
        let o3 = t.on_stt(&fin(Stream::System, 22, 26, "Теперь следующий вопрос."));
        assert_eq!(o3.closed.len(), 1);
        assert_eq!(o3.closed[0].reason, CloseReason::SystemQuestion);
        assert_eq!(o3.closed[0].text, "Это прибыль до налогов.");
        assert_eq!(o3.closed[0].first_at, Some(12 * S));
        assert!(t.open_qa().is_none());
        // Closed link no longer collects.
        t.on_stt(&fin(Stream::Me, 30, 31, "поздно"));
        assert_eq!(t.my_answer(&qa(1)), Some("Это прибыль до налогов."));
    }

    #[test]
    fn long_system_without_me_segment_keeps_link_open() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        let o = t.on_stt(&fin(Stream::System, 1, 8, "длинная реплика"));
        assert!(o.closed.is_empty());
        assert_eq!(t.open_qa(), Some(qa(1)));
    }

    #[test]
    fn times_out_after_90s_on_event_or_tick() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        t.on_stt(&fin(Stream::Me, 1, 2, "раз"));
        assert!(t.tick(90 * S).closed.is_empty());
        let o = t.tick(91 * S);
        assert_eq!(o.closed[0].reason, CloseReason::Timeout);

        // Event path: a Me final after the deadline is not collected.
        t.arm(qa(2), "q".into(), "h".into(), 100 * S);
        t.on_stt(&fin(Stream::Me, 101, 102, "два"));
        let o = t.on_stt(&fin(Stream::Me, 195, 196, "слишком поздно"));
        assert_eq!(o.closed[0].text, "два");
        assert_eq!(t.my_answer(&qa(2)), Some("два"));
    }

    #[test]
    fn next_ask_and_rearm_close_previous_link() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q1".into(), "h1".into(), 0);
        t.on_stt(&fin(Stream::Me, 1, 2, "ответ один"));
        let o = t.on_ask();
        assert_eq!(o.closed[0].reason, CloseReason::NextAsk);
        // Arming while another is open closes it too.
        t.arm(qa(2), "q2".into(), "h2".into(), 10 * S);
        t.on_stt(&fin(Stream::Me, 11, 12, "ответ два"));
        let o = t.arm(qa(3), "q3".into(), "h3".into(), 20 * S);
        assert_eq!(o.closed[0].qa_id, qa(2));
        assert_eq!(t.latest_my_answer(), Some("ответ два"));
        assert_eq!(t.records().len(), 3);
        assert_eq!(t.open_qa(), Some(qa(3)));
    }

    #[test]
    fn empty_answer_closes_without_journal_entry() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        let o = t.close(CloseReason::SessionEnd);
        assert!(o.closed.is_empty());
        assert_eq!(o.updates[0].closed, Some(CloseReason::SessionEnd));
    }

    #[test]
    fn partials_are_ignored() {
        let mut t = QaTracker::default();
        t.arm(qa(1), "q".into(), "h".into(), 0);
        let mut p = fin(Stream::Me, 1, 2, "ну");
        p.kind = SttKind::Partial;
        assert!(t.on_stt(&p).updates.is_empty());
    }
}
