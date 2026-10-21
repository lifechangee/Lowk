//! Transcript-level echo guard (S4 safety net, docs/research/audio-capture.md §3).
//!
//! Without headphones the speakers play the System stream and the microphone hears it again,
//! so the same sentence arrives as «Система» and as «Я». AEC removes most of it; this drops what
//! is left: a Me sentence that fuzzy-matches System speech heard around the same audio time
//! never reaches the transcript, the journal or «Мой ответ». Real double-talk survives because
//! the test is per sentence and needs most of the words to be explained by System speech.
use lowk_types::{MonoNs, Stream, SttEvent};
use std::collections::VecDeque;

/// System speech this far before / after the Me utterance can explain it.
const BEFORE_NS: u64 = 3_000_000_000;
const AFTER_NS: u64 = 5_000_000_000;
/// Remember System speech this long (audio time).
const KEEP_NS: u64 = 60_000_000_000;
/// Share of a Me sentence's words found in System speech to call it echo.
const COVERAGE: f32 = 0.6;
/// Words shorter than this must match exactly; longer ones fuzzily.
const FUZZY_MIN: usize = 4;
const FUZZY_SIM: f32 = 0.72;

#[derive(Default)]
pub struct EchoGuard {
    /// (utt, t_start, t_end, words) of System utterances, newest text per utt.
    sys: VecDeque<(u64, MonoNs, MonoNs, Vec<String>)>,
}

pub fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .replace('ё', "е")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

fn lev(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1)
                .min(cur[j - 1] + 1)
                .min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn same_word(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (ca, cb): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if ca.len().min(cb.len()) < FUZZY_MIN {
        return false;
    }
    let m = ca.len().max(cb.len());
    1.0 - lev(&ca, &cb) as f32 / m as f32 >= FUZZY_SIM
}

/// Sentences of `text`, each with its trailing punctuation.
fn sentences(text: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '?' | '!' | '…') {
            let end = i + c.len_utf8();
            if !text[start..end].trim().is_empty() {
                out.push(text[start..end].trim());
            }
            start = end;
        }
    }
    if !text[start..].trim().is_empty() {
        out.push(text[start..].trim());
    }
    out
}

impl EchoGuard {
    /// Records System speech (partials and finals; the newest text of an utterance wins).
    pub fn note_system(&mut self, ev: &SttEvent) {
        if ev.stream != Stream::System {
            return;
        }
        let w = words(&ev.text);
        if let Some(e) = self.sys.iter_mut().find(|e| e.0 == ev.utt) {
            e.2 = e.2.max(ev.t_audio_end_ns);
            e.3 = w;
        } else {
            self.sys.push_back((ev.utt, ev.t_start_ns, ev.t_audio_end_ns, w));
        }
        while self
            .sys
            .front()
            .is_some_and(|f| f.2 + KEEP_NS < ev.t_audio_end_ns)
        {
            self.sys.pop_front();
        }
    }

    fn is_echo(&self, sentence: &str, t0: MonoNs, t1: MonoNs) -> bool {
        let mine = words(sentence);
        if mine.len() < 2 {
            return false;
        }
        let pool: Vec<&String> = self
            .sys
            .iter()
            .filter(|e| e.2 + BEFORE_NS >= t0 && e.1 <= t1 + AFTER_NS)
            .flat_map(|e| e.3.iter())
            .collect();
        if pool.is_empty() {
            return false;
        }
        let hit = mine
            .iter()
            .filter(|w| pool.iter().any(|p| same_word(w, p)))
            .count();
        hit as f32 / mine.len() as f32 >= COVERAGE
    }

    /// The Me event without its echoed sentences; `None` when nothing real is left.
    pub fn filter_me(&self, ev: &SttEvent) -> Option<SttEvent> {
        if ev.stream != Stream::Me {
            return Some(ev.clone());
        }
        let kept: Vec<&str> = sentences(&ev.text)
            .into_iter()
            .filter(|s| !self.is_echo(s, ev.t_start_ns, ev.t_audio_end_ns))
            .collect();
        if kept.is_empty() {
            return None;
        }
        let mut out = ev.clone();
        out.text = kept.join(" ");
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::SttKind;

    const S: u64 = 1_000_000_000;
    fn ev(stream: Stream, utt: u64, a: u64, b: u64, text: &str) -> SttEvent {
        SttEvent {
            stream,
            utt,
            rev: 0,
            kind: SttKind::Final,
            text: text.into(),
            lang: None,
            t_start_ns: a * S,
            t_audio_end_ns: b * S,
            t_emit_ns: b * S,
        }
    }

    #[test]
    fn s4_echo_of_system_speech_is_dropped_even_with_asr_differences() {
        let mut g = EchoGuard::default();
        g.note_system(&ev(
            Stream::System,
            1,
            0,
            6,
            "Привет, рад познакомиться. Расскажи коротко, чем занимался последние два года.",
        ));
        // the mic hears it again, transcribed slightly differently, a bit later
        let echo = ev(
            Stream::Me,
            1,
            1,
            7,
            "Привет, раз познакомиться. Расскажи коротко и чем занимался последние два года.",
        );
        assert!(g.filter_me(&echo).is_none());
    }

    #[test]
    fn s4_real_speech_survives_and_mixed_utterances_lose_only_the_echo() {
        let mut g = EchoGuard::default();
        g.note_system(&ev(
            Stream::System,
            1,
            0,
            4,
            "Расскажи кейс, где метрики не сошлись с ожиданиями.",
        ));
        let own = ev(Stream::Me, 1, 10, 14, "Мы запускали новый флоу, retention упал.");
        assert_eq!(g.filter_me(&own).unwrap().text, own.text);
        let mixed = ev(
            Stream::Me,
            2,
            1,
            12,
            "Расскажи кейс, где метрики не сошлись с ожиданиями. Да, был такой случай на прошлой работе.",
        );
        assert_eq!(
            g.filter_me(&mixed).unwrap().text,
            "Да, был такой случай на прошлой работе."
        );
    }

    #[test]
    fn s4_time_alignment_and_short_utterances() {
        let mut g = EchoGuard::default();
        g.note_system(&ev(Stream::System, 1, 0, 3, "Хорошо, давай сразу к цифрам."));
        // same words but minutes later: the user repeating on purpose is kept
        assert!(g
            .filter_me(&ev(Stream::Me, 1, 300, 303, "Хорошо, давай сразу к цифрам."))
            .is_some());
        // a single word is never judged
        assert!(g.filter_me(&ev(Stream::Me, 2, 1, 2, "Хорошо.")).is_some());
        // partials are judged with the same rule
        let mut p = ev(Stream::Me, 3, 1, 2, "Хорошо, давай сразу");
        p.kind = SttKind::Partial;
        assert!(g.filter_me(&p).is_none());
    }
}
