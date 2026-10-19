//! Replay / demo feed: a scripted, deterministic RU/EN call with partials,
//! finals (code-switching), two AI hints and the user's «Мой ответ».
//! The script is pure data on a virtual timeline (ms from call start); the
//! session rebases it onto the monotonic clock when it plays it.
use lowk_types::{MonoNs, Stream, SttEvent, SttKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum ReplayKind {
    /// Timestamps inside are virtual (ns from call start).
    Stt(SttEvent),
    /// The user pressed Ask; `qa` is the script-local question number.
    HintStart {
        qa: u32,
        question: String,
    },
    HintDelta {
        qa: u32,
        text: String,
    },
    HintDone {
        qa: u32,
        answer: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplayItem {
    pub at_ms: u64,
    pub kind: ReplayKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplayScript {
    items: Vec<ReplayItem>,
}

/// Adds `t0` to every timestamp of a virtual-time event.
pub fn rebase(ev: &SttEvent, t0: MonoNs) -> SttEvent {
    let mut e = ev.clone();
    e.t_start_ns += t0;
    e.t_audio_end_ns += t0;
    e.t_emit_ns += t0;
    e
}

struct Utt {
    stream: Stream,
    start_ms: u64,
    text: &'static str,
    ms_per_word: u64,
    /// (word index, wrong text) shown in partials only; the final is correct.
    mishear: Option<(usize, &'static str)>,
    lang: &'static str,
}

const NS: u64 = 1_000_000;

impl ReplayScript {
    pub fn items(&self) -> &[ReplayItem] {
        &self.items
    }
    pub fn duration_ms(&self) -> u64 {
        self.items.last().map_or(0, |i| i.at_ms)
    }

    /// The built-in demo: a RU interview with English terms, ~60 s.
    pub fn demo() -> Self {
        use Stream::{Me, System};
        let utts = [
            Utt { stream: System, start_ms: 600, ms_per_word: 330, mishear: None, lang: "ru",
                text: "Привет! Спасибо, что нашли время, давайте начнём с вашего опыта в финтехе." },
            Utt { stream: Me, start_ms: 2600, ms_per_word: 300, mishear: None, lang: "ru", text: "Ага." },
            Utt { stream: Me, start_ms: 5600, ms_per_word: 300, mishear: None, lang: "ru",
                text: "Да, конечно. Последние четыре года я строил data-платформу для расчёта кредитных рейтингов, в основном на Spark и Airflow." },
            Utt { stream: System, start_ms: 12800, ms_per_word: 330, mishear: Some((4, "ибитда,")), lang: "ru",
                text: "Отлично. А что значит EBITDA, если по-простому?" },
            Utt { stream: Me, start_ms: 19000, ms_per_word: 300, mishear: None, lang: "ru",
                text: "EBITDA — это операционная прибыль до вычета процентов, налогов и амортизации." },
            Utt { stream: Me, start_ms: 26500, ms_per_word: 300, mishear: None, lang: "ru",
                text: "Ну то есть по сути cash-proxy, чтобы сравнивать компании независимо от долга." },
            Utt { stream: System, start_ms: 32000, ms_per_word: 330, mishear: None, lang: "ru",
                text: "Понятно, спасибо. Теперь вторая часть: how do you usually handle a churn spike right after a rollout? Расскажите на конкретном примере, что вы делали в первые две недели." },
            Utt { stream: Me, start_ms: 45500, ms_per_word: 300, mishear: None, lang: "ru",
                text: "Конечно. Мы сначала смотрели на cohort retention, потом разбирали причины по сегментам и подняли alert в Grafana." },
            Utt { stream: System, start_ms: 53500, ms_per_word: 330, mishear: None, lang: "ru",
                text: "Хорошо, это именно то, что мы хотели услышать, особенно про алерты и разбор по сегментам, нам это близко." },
            Utt { stream: Me, start_ms: 60000, ms_per_word: 300, mishear: None, lang: "ru",
                text: "Спасибо! У меня тоже есть пара вопросов про команду." },
        ];
        let mut items: Vec<ReplayItem> = vec![];
        let mut next_utt = [0u64; 2];
        let idx = |s: Stream| if s == Stream::Me { 0 } else { 1 };
        for u in &utts {
            next_utt[idx(u.stream)] += 1;
            let utt = next_utt[idx(u.stream)];
            let words: Vec<&str> = u.text.split_whitespace().collect();
            let n = words.len();
            let speech_start = u.start_ms + 250;
            let mut rev = 0u32;
            let mut k = 2;
            while k < n {
                let mut shown: Vec<&str> = words[..k].to_vec();
                if let Some((i, w)) = u.mishear {
                    if i < k {
                        shown[i] = w;
                    }
                }
                let end = speech_start + k as u64 * u.ms_per_word;
                items.push(stt_item(
                    u,
                    utt,
                    rev,
                    SttKind::Partial,
                    shown.join(" "),
                    end,
                    end + 120,
                ));
                rev += 1;
                k += 2;
            }
            let end = speech_start + n as u64 * u.ms_per_word;
            items.push(stt_item(
                u,
                utt,
                rev,
                SttKind::Final,
                u.text.to_string(),
                end,
                end + 180,
            ));
        }
        // Hints: Ask is pressed shortly after the question's final.
        for (qa, ask_ms, question, answer) in [
            (
                1u32,
                16_000u64,
                "А что значит EBITDA, если по-простому?",
                "EBITDA — прибыль до вычета процентов, налогов и амортизации. Показывает операционную доходность бизнеса без учёта долговой нагрузки.",
            ),
            (
                2,
                43_000,
                "How do you usually handle a churn spike right after a rollout?",
                "Churn spike после rollout: сначала сегментировать по cohort и каналу, проверить релизные изменения, затем включить алерты и быстрый rollback.",
            ),
        ] {
            items.push(ReplayItem { at_ms: ask_ms, kind: ReplayKind::HintStart { qa, question: question.into() } });
            let mut t = ask_ms + 300;
            for w in answer.split_inclusive(' ') {
                items.push(ReplayItem { at_ms: t, kind: ReplayKind::HintDelta { qa, text: w.into() } });
                t += 70;
            }
            items.push(ReplayItem { at_ms: t, kind: ReplayKind::HintDone { qa, answer: answer.into() } });
        }
        // Stable sort keeps generation order for equal timestamps.
        items.sort_by_key(|i| i.at_ms);
        Self { items }
    }
}

fn stt_item(
    u: &Utt,
    utt: u64,
    rev: u32,
    kind: SttKind,
    text: String,
    audio_end_ms: u64,
    emit_ms: u64,
) -> ReplayItem {
    ReplayItem {
        at_ms: emit_ms,
        kind: ReplayKind::Stt(SttEvent {
            stream: u.stream,
            utt,
            rev,
            kind,
            text,
            lang: Some(u.lang.into()),
            t_start_ns: (u.start_ms + 250) * NS,
            t_audio_end_ns: audio_end_ms * NS,
            t_emit_ns: emit_ms * NS,
        }),
    }
}

/// How fast a script is played.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DemoPace {
    /// 1.0 = real time.
    Realtime(f32),
    /// No sleeping (tests).
    Instant,
}

/// Sleeps on the virtual timeline; honours cancel and pause.
#[derive(Clone)]
pub struct Pacer {
    pub cancel: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
    pub pace: DemoPace,
}

impl Pacer {
    /// Advance virtual time by `ms`. Returns false when cancelled.
    fn advance(&self, ms: u64) -> bool {
        let mut left = match self.pace {
            DemoPace::Instant => 0.0,
            DemoPace::Realtime(s) => ms as f64 / f64::from(s.max(0.01)),
        };
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return false;
            }
            if self.paused.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            if left <= 0.0 {
                return true;
            }
            let step = left.min(10.0);
            std::thread::sleep(Duration::from_micros((step * 1000.0) as u64));
            left -= step;
        }
    }
}

pub struct ReplaySource {
    script: Arc<ReplayScript>,
}

impl ReplaySource {
    pub fn new(script: Arc<ReplayScript>) -> Self {
        Self { script }
    }
    pub fn demo() -> Self {
        Self::new(Arc::new(ReplayScript::demo()))
    }
    pub fn script(&self) -> &ReplayScript {
        &self.script
    }

    /// Plays every item in order, calling `out`. Returns true if it ran to the
    /// end, false if cancelled.
    pub fn run(&self, pacer: &Pacer, out: &mut dyn FnMut(&ReplayItem)) -> bool {
        let mut now = 0;
        for item in self.script.items() {
            if item.at_ms > now {
                if !pacer.advance(item.at_ms - now) {
                    return false;
                }
                now = item.at_ms;
            } else if pacer.cancel.load(Ordering::SeqCst) {
                return false;
            }
            out(item);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant() -> Pacer {
        Pacer {
            cancel: Arc::default(),
            paused: Arc::default(),
            pace: DemoPace::Instant,
        }
    }

    #[test]
    fn demo_script_is_deterministic() {
        assert_eq!(ReplayScript::demo(), ReplayScript::demo());
        let mut a = vec![];
        let mut b = vec![];
        ReplaySource::demo().run(&instant(), &mut |i| a.push(i.clone()));
        ReplaySource::demo().run(&instant(), &mut |i| b.push(i.clone()));
        assert_eq!(a, b);
        assert_eq!(a.len(), ReplayScript::demo().items().len());
    }

    #[test]
    fn demo_is_time_ordered_and_realistic() {
        let s = ReplayScript::demo();
        assert!(s.items().windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
        assert!(s.duration_ms() > 55_000);
        let stt: Vec<&SttEvent> = s
            .items()
            .iter()
            .filter_map(|i| match &i.kind {
                ReplayKind::Stt(e) => Some(e),
                _ => None,
            })
            .collect();
        assert!(stt.iter().any(|e| e.kind == SttKind::Partial));
        assert!(stt
            .iter()
            .any(|e| e.text.contains("EBITDA") && e.kind == SttKind::Final));
        // Code-switching: Latin and Cyrillic in the same final.
        assert!(stt.iter().any(|e| e.kind == SttKind::Final
            && e.text.chars().any(|c| c.is_ascii_alphabetic())
            && e.text
                .chars()
                .any(|c| ('а'..='я').contains(&c.to_ascii_lowercase()))));
        // Revisions are monotonic per utterance and every utterance ends in one final.
        for st in [Stream::Me, Stream::System] {
            let max = stt
                .iter()
                .filter(|e| e.stream == st)
                .map(|e| e.utt)
                .max()
                .unwrap();
            for u in 1..=max {
                let revs: Vec<_> = stt.iter().filter(|e| e.stream == st && e.utt == u).collect();
                assert!(revs.windows(2).all(|w| w[0].rev < w[1].rev));
                assert_eq!(revs.last().unwrap().kind, SttKind::Final);
                assert_eq!(revs.iter().filter(|e| e.kind == SttKind::Final).count(), 1);
            }
        }
        // The misheard partial is corrected by the final.
        assert!(stt
            .iter()
            .any(|e| e.kind == SttKind::Partial && e.text.contains("ибитда")));
        // A hint follows each ask.
        let dones = s
            .items()
            .iter()
            .filter(|i| matches!(i.kind, ReplayKind::HintDone { .. }))
            .count();
        assert_eq!(dones, 2);
    }

    #[test]
    fn cancel_stops_replay() {
        let p = instant();
        p.cancel.store(true, Ordering::SeqCst);
        let mut n = 0;
        assert!(!ReplaySource::demo().run(&p, &mut |_| n += 1));
        assert_eq!(n, 0);
    }
}
