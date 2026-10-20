//! In-memory live transcript: one line per (stream, utterance), revised in
//! place. `seq` is assigned when an utterance is first seen and never changes,
//! so the UI can patch lines by seq and a lagging subscriber can resync.
use lowk_types::{SessionClock, Stream, SttEvent, SttKind, TranscriptLine, UttId};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    Inserted(u64),
    Updated(u64),
    /// Older revision, duplicate, or a partial after the final: ignored.
    Stale,
}

#[derive(Clone, Debug)]
struct Entry {
    line: TranscriptLine,
    rev: u32,
    t_start_ns: u64,
}

#[derive(Default)]
pub struct TranscriptBuf {
    entries: Vec<Entry>,
    index: HashMap<(Stream, UttId), usize>,
    next_seq: u64,
}

impl TranscriptBuf {
    pub fn apply(&mut self, ev: &SttEvent, clock: &SessionClock) -> Applied {
        let is_final = ev.kind == SttKind::Final;
        if let Some(&i) = self.index.get(&(ev.stream, ev.utt)) {
            let e = &mut self.entries[i];
            let accept = if e.line.is_final {
                // Only a strictly newer final may revise a final line.
                is_final && ev.rev > e.rev
            } else if is_final {
                ev.rev >= e.rev
            } else {
                ev.rev > e.rev
            };
            if !accept {
                return Applied::Stale;
            }
            e.rev = ev.rev;
            e.line.text = ev.text.clone();
            e.line.is_final = is_final;
            return Applied::Updated(e.line.seq);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.index.insert((ev.stream, ev.utt), self.entries.len());
        self.entries.push(Entry {
            line: TranscriptLine {
                seq,
                stream: ev.stream,
                offset_s: (clock.offset_ns(ev.t_start_ns) / 1_000_000_000) as u32,
                text: ev.text.clone(),
                is_final,
            },
            rev: ev.rev,
            t_start_ns: ev.t_start_ns,
        });
        Applied::Inserted(seq)
    }

    pub fn lines(&self) -> Vec<TranscriptLine> {
        self.entries.iter().map(|e| e.line.clone()).collect()
    }

    /// Lines with `seq >= from_seq` (the LLM cursor: first seq not yet sent).
    pub fn since(&self, from_seq: u64) -> Vec<TranscriptLine> {
        self.entries
            .iter()
            .filter(|e| e.line.seq >= from_seq)
            .map(|e| e.line.clone())
            .collect()
    }

    /// What an Ask sends: everything since `from_seq`, **including the utterance that is
    /// still being spoken** (its current partial text). Never waits for a final; the
    /// partial is whatever the engine has shown on screen at this instant.
    pub fn snapshot_for_ask(&self, from_seq: u64) -> Vec<TranscriptLine> {
        self.since(from_seq)
            .into_iter()
            .filter(|l| !l.text.trim().is_empty())
            .collect()
    }

    /// Seq the next new utterance will get.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    pub fn get(&self, stream: Stream, utt: UttId) -> Option<&TranscriptLine> {
        self.index.get(&(stream, utt)).map(|&i| &self.entries[i].line)
    }

    /// Start time (monotonic ns) of the line with this seq.
    pub fn t_start(&self, seq: u64) -> Option<u64> {
        self.entries
            .iter()
            .find(|e| e.line.seq == seq)
            .map(|e| e.t_start_ns)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock() -> SessionClock {
        SessionClock {
            t0_mono: 1_000_000_000,
            t0_wall_ms: 0,
        }
    }
    fn ev(stream: Stream, utt: UttId, rev: u32, kind: SttKind, text: &str) -> SttEvent {
        SttEvent {
            stream,
            utt,
            rev,
            kind,
            text: text.into(),
            lang: None,
            t_start_ns: 4_000_000_000,
            t_audio_end_ns: 5_000_000_000,
            t_emit_ns: 5_000_000_000,
        }
    }

    #[test]
    fn partials_revise_in_place_and_keep_seq() {
        let mut t = TranscriptBuf::default();
        let c = clock();
        let p = SttKind::Partial;
        assert_eq!(
            t.apply(&ev(Stream::System, 1, 0, p, "при"), &c),
            Applied::Inserted(0)
        );
        assert_eq!(
            t.apply(&ev(Stream::System, 1, 1, p, "привет"), &c),
            Applied::Updated(0)
        );
        assert_eq!(t.apply(&ev(Stream::Me, 1, 0, p, "да"), &c), Applied::Inserted(1));
        let l = t.lines();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].text, "привет");
        assert_eq!(l[0].offset_s, 3);
        assert!(!l[0].is_final);
    }

    #[test]
    fn stale_and_duplicate_revisions_are_ignored() {
        let mut t = TranscriptBuf::default();
        let c = clock();
        let p = SttKind::Partial;
        t.apply(&ev(Stream::Me, 1, 2, p, "три слова тут"), &c);
        assert_eq!(t.apply(&ev(Stream::Me, 1, 1, p, "три"), &c), Applied::Stale);
        assert_eq!(t.apply(&ev(Stream::Me, 1, 2, p, "dup"), &c), Applied::Stale);
        assert_eq!(t.lines()[0].text, "три слова тут");
    }

    #[test]
    fn final_closes_line_and_blocks_late_partials() {
        let mut t = TranscriptBuf::default();
        let c = clock();
        t.apply(&ev(Stream::Me, 1, 0, SttKind::Partial, "ну"), &c);
        // A final may reuse the last partial rev.
        let f = SttKind::Final;
        assert_eq!(
            t.apply(&ev(Stream::Me, 1, 0, f, "Ну да."), &c),
            Applied::Updated(0)
        );
        assert!(t.lines()[0].is_final);
        // A late partial (out-of-order delivery) cannot reopen it.
        let late = ev(Stream::Me, 1, 5, SttKind::Partial, "ну да ладно");
        assert_eq!(t.apply(&late, &c), Applied::Stale);
        // Same-rev final again: stale; strictly newer final: revises.
        assert_eq!(t.apply(&ev(Stream::Me, 1, 0, f, "x"), &c), Applied::Stale);
        assert_eq!(
            t.apply(&ev(Stream::Me, 1, 1, f, "Ну да!"), &c),
            Applied::Updated(0)
        );
        assert_eq!(t.lines()[0].text, "Ну да!");
        assert!(t.lines()[0].is_final);
    }

    #[test]
    fn same_utt_id_on_different_streams_is_independent() {
        let mut t = TranscriptBuf::default();
        let c = clock();
        t.apply(&ev(Stream::Me, 1, 0, SttKind::Final, "a"), &c);
        t.apply(&ev(Stream::System, 1, 0, SttKind::Final, "b"), &c);
        assert_eq!(t.len(), 2);
        assert_eq!(t.get(Stream::System, 1).unwrap().text, "b");
    }

    #[test]
    fn snapshot_for_ask_includes_open_partials_immediately() {
        let mut t = TranscriptBuf::default();
        let c = clock();
        t.apply(&ev(Stream::System, 1, 0, SttKind::Final, "Расскажи про проект."), &c);
        t.apply(&ev(Stream::System, 2, 0, SttKind::Partial, "а какие были метр"), &c);
        let s = t.snapshot_for_ask(0);
        assert_eq!(s.len(), 2);
        assert!(!s[1].is_final);
        assert_eq!(s[1].text, "а какие были метр");
        // a correction that arrives later replaces the stored text but not what was sent
        t.apply(&ev(Stream::System, 1, 1, SttKind::Final, "Расскажи про проект!"), &c);
        assert_eq!(t.get(Stream::System, 1).unwrap().text, "Расскажи про проект!");
    }

    #[test]
    fn since_returns_tail_for_llm_cursor() {
        let mut t = TranscriptBuf::default();
        let c = clock();
        for u in 1..=4 {
            t.apply(&ev(Stream::Me, u, 0, SttKind::Final, "x"), &c);
        }
        assert_eq!(t.since(2).len(), 2);
        assert_eq!(t.since(t.next_seq()).len(), 0);
    }
}
