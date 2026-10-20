//! Cloud realtime engines over WebSocket: Soniox (default) and Deepgram Nova-3 multilingual.
//! One connection per stream. Keys come from the OS keychain (`stt-cloud`); for development
//! the `SONIOX_API_KEY` / `DEEPGRAM_API_KEY` environment variables or `~/.lowk-test-keys`
//! are honoured (debug builds, or `LOWK_DEV_KEYS=1`). Keys are never logged.
use crate::models::DownloadSpec;
use crate::{SttEngine, SttEngineFactory};
use lowk_types::*;
use serde_json::Value;

#[path = "cloud_ws.rs"]
mod cloud_ws;
pub(crate) use cloud_ws::tls_provider;
pub use cloud_ws::CloudEngine;

pub const SONIOX_ID: &str = "soniox";
pub const DEEPGRAM_ID: &str = "deepgram";
pub(crate) const SONIOX_URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
pub(crate) const DEEPGRAM_HOST: &str = "wss://api.deepgram.com/v1/listen";
pub(crate) const SONIOX_MODEL: &str = "stt-rt-v5";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Soniox,
    Deepgram,
}

impl Vendor {
    pub fn id(self) -> &'static str {
        match self {
            Vendor::Soniox => SONIOX_ID,
            Vendor::Deepgram => DEEPGRAM_ID,
        }
    }
    fn env(self) -> &'static str {
        match self {
            Vendor::Soniox => "SONIOX_API_KEY",
            Vendor::Deepgram => "DEEPGRAM_API_KEY",
        }
    }
}

fn dev_keys_allowed() -> bool {
    cfg!(debug_assertions) || std::env::var_os("LOWK_DEV_KEYS").is_some()
}

/// Environment, then (dev only) `~/.lowk-test-keys`, then the OS keychain (`stt-cloud`).
pub fn resolve_key(v: Vendor) -> Result<String> {
    if dev_keys_allowed() {
        if let Ok(k) = std::env::var(v.env()) {
            if !k.is_empty() {
                return Ok(k);
            }
        }
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            if let Ok(s) = std::fs::read_to_string(std::path::Path::new(&home).join(".lowk-test-keys")) {
                for l in s.lines() {
                    let l = l.trim().trim_start_matches("export ").trim();
                    if let Some(val) = l.strip_prefix(&format!("{}=", v.env())) {
                        let val = val.trim().trim_matches(|c| c == '"' || c == '\'');
                        if !val.is_empty() {
                            return Ok(val.to_owned());
                        }
                    }
                }
            }
        }
    }
    lowk_platform::secrets::get("stt-cloud")
}

// ---------------------------------------------------------------------------------------
// Protocol parsers (pure; unit-tested against canned vendor messages)
// ---------------------------------------------------------------------------------------

/// Turns vendor JSON into utterance revisions. `t0` is the capture time of the first
/// audio frame sent on this connection (vendor timestamps are relative to it).
pub struct Parser {
    vendor: Vendor,
    stream: Stream,
    pub t0: Option<MonoNs>,
    utt_base: u64,
    utt: u64,
    rev: u32,
    text: String,
    last_partial: String,
    t_start: Option<MonoNs>,
    t_end: MonoNs,
    lang: Option<String>,
    pub finished: bool,
}

impl Parser {
    pub fn new(vendor: Vendor, stream: Stream, utt_base: u64) -> Self {
        Self {
            vendor,
            stream,
            t0: None,
            utt_base,
            utt: 1,
            rev: 0,
            text: String::new(),
            last_partial: String::new(),
            t_start: None,
            t_end: 0,
            lang: None,
            finished: false,
        }
    }

    fn at(&self, ms: f64) -> MonoNs {
        self.t0.unwrap_or(0) + (ms.max(0.0) * 1_000_000.0) as u64
    }

    fn mk(&self, kind: SttKind, text: String, now: MonoNs) -> SttEvent {
        SttEvent {
            stream: self.stream,
            utt: self.utt_base + self.utt,
            rev: self.rev,
            kind,
            text,
            lang: self.lang.clone(),
            t_start_ns: self.t_start.unwrap_or(self.t_end),
            t_audio_end_ns: self.t_end.max(self.t_start.unwrap_or(0)),
            t_emit_ns: now,
        }
    }

    fn close_utt(&mut self, out: &mut Vec<SttEvent>, now: MonoNs) {
        let t = self.text.trim().to_owned();
        if !t.is_empty() {
            out.push(self.mk(SttKind::Final, t, now));
            self.utt += 1;
        }
        self.rev = 0;
        self.text.clear();
        self.last_partial.clear();
        self.t_start = None;
    }

    fn partial(&mut self, interim: &str, out: &mut Vec<SttEvent>, now: MonoNs) {
        let full = format!("{}{}", self.text, interim);
        let full = full.trim().to_owned();
        if !full.is_empty() && full != self.last_partial {
            self.last_partial = full.clone();
            out.push(self.mk(SttKind::Partial, full, now));
            self.rev += 1;
        }
    }

    /// Ends the open utterance (flush / end of stream).
    pub fn flush(&mut self, now: MonoNs) -> Vec<SttEvent> {
        let mut out = vec![];
        self.close_utt(&mut out, now);
        out
    }

    pub fn on_message(&mut self, raw: &str, now: MonoNs) -> Vec<SttEvent> {
        let Ok(v) = serde_json::from_str::<Value>(raw) else {
            return vec![];
        };
        match self.vendor {
            Vendor::Soniox => self.soniox(&v, now),
            Vendor::Deepgram => self.deepgram(&v, now),
        }
    }

    fn soniox(&mut self, v: &Value, now: MonoNs) -> Vec<SttEvent> {
        let mut out = vec![];
        if v["finished"] == true {
            self.finished = true;
        }
        let mut interim = String::new();
        for t in v["tokens"].as_array().into_iter().flatten() {
            let text = t["text"].as_str().unwrap_or("");
            let fin = t["is_final"] == true;
            if let Some(e) = t["end_ms"].as_f64() {
                self.t_end = self.t_end.max(self.at(e));
            }
            if text == "<end>" || text == "<fin>" {
                if fin {
                    self.close_utt(&mut out, now);
                }
                continue;
            }
            if self.t_start.is_none() {
                if let Some(s) = t["start_ms"].as_f64() {
                    self.t_start = Some(self.at(s));
                }
            }
            if let Some(l) = t["language"].as_str() {
                self.lang = Some(l.to_owned());
            }
            if fin {
                self.text.push_str(text);
            } else {
                interim.push_str(text);
            }
        }
        self.partial(&interim, &mut out, now);
        out
    }

    fn deepgram(&mut self, v: &Value, now: MonoNs) -> Vec<SttEvent> {
        let mut out = vec![];
        match v["type"].as_str() {
            Some("Results") => {
                let alt = &v["channel"]["alternatives"][0];
                let tr = alt["transcript"].as_str().unwrap_or("").trim();
                let start = v["start"].as_f64().unwrap_or(0.0);
                let dur = v["duration"].as_f64().unwrap_or(0.0);
                self.t_end = self.t_end.max(self.at((start + dur) * 1000.0));
                if !tr.is_empty() && self.t_start.is_none() {
                    self.t_start = Some(self.at(start * 1000.0));
                }
                if let Some(l) = alt["languages"][0].as_str() {
                    self.lang = Some(l.to_owned());
                }
                if v["is_final"] == true {
                    if !tr.is_empty() {
                        if !self.text.is_empty() {
                            self.text.push(' ');
                        }
                        self.text.push_str(tr);
                    }
                    self.partial("", &mut out, now);
                    if v["speech_final"] == true {
                        self.close_utt(&mut out, now);
                    }
                } else if !tr.is_empty() {
                    let sep = if self.text.is_empty() { "" } else { " " };
                    self.partial(&format!("{sep}{tr}"), &mut out, now);
                }
            }
            Some("UtteranceEnd") => self.close_utt(&mut out, now),
            _ => {}
        }
        out
    }
}

pub(crate) fn deepgram_url(langs: &[LangTag]) -> String {
    let lang = match langs {
        [one] => one.as_str(),
        _ => "multi",
    };
    format!(
        "{DEEPGRAM_HOST}?model=nova-3&language={lang}&encoding=linear16&sample_rate=16000&channels=1\
         &interim_results=true&smart_format=true&punctuate=true&endpointing=400&utterance_end_ms=1000"
    )
}

pub fn descriptor(v: Vendor) -> EngineDescriptor {
    let (name, langs): (&str, &[&str]) = match v {
        Vendor::Soniox => (
            "Soniox (cloud)",
            &[
                "ru", "en", "de", "fr", "es", "it", "pt", "uk", "pl", "tr", "ja", "ko", "zh",
            ],
        ),
        Vendor::Deepgram => (
            "Deepgram Nova-3 (cloud)",
            &["ru", "en", "de", "fr", "es", "it", "pt", "nl", "ja", "hi"],
        ),
    };
    EngineDescriptor {
        id: v.id().into(),
        name: name.into(),
        kind: EngineKind::Cloud,
        languages: langs.iter().map(|s| s.to_string()).collect(),
        streaming: StreamingMode::Native,
        size_mb: 0,
        license: "Proprietary service".into(),
    }
}

pub struct CloudFactory(pub Vendor);

impl SttEngineFactory for CloudFactory {
    fn descriptor(&self) -> EngineDescriptor {
        descriptor(self.0)
    }
    fn fitness(&self, _hw: &HwInfo) -> Fitness {
        Fitness::Possible
    }
    fn create(&self, stream: Stream) -> Result<Box<dyn SttEngine>> {
        Ok(Box::new(CloudEngine::new(self.0, stream)))
    }
    fn assets(&self) -> Vec<DownloadSpec> {
        vec![]
    }
    fn needs_key(&self) -> bool {
        true
    }
    fn license_url(&self) -> String {
        match self.0 {
            Vendor::Soniox => "https://soniox.com/terms".into(),
            Vendor::Deepgram => "https://deepgram.com/terms".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: MonoNs = 10_000_000_000;

    fn texts(v: &[SttEvent]) -> Vec<(SttKind, u64, u32, &str)> {
        v.iter()
            .map(|e| (e.kind, e.utt, e.rev, e.text.as_str()))
            .collect()
    }

    #[test]
    fn soniox_partials_endpoint_and_next_utterance() {
        let mut p = Parser::new(Vendor::Soniox, Stream::System, 0);
        p.t0 = Some(T0);
        let a = p.on_message(
            r#"{"tokens":[{"text":"Привет","start_ms":100,"end_ms":500,"is_final":true,"language":"ru"},{"text":" как","start_ms":600,"end_ms":800,"is_final":false}]}"#,
            1,
        );
        assert_eq!(texts(&a), vec![(SttKind::Partial, 1, 0, "Привет как")]);
        assert_eq!(a[0].t_start_ns, T0 + 100_000_000);
        assert_eq!(a[0].t_audio_end_ns, T0 + 800_000_000);
        assert_eq!(a[0].lang.as_deref(), Some("ru"));
        let b = p.on_message(
            r#"{"tokens":[{"text":" как дела","start_ms":600,"end_ms":1200,"is_final":true},{"text":"<end>","is_final":true},{"text":"Ок","start_ms":1500,"end_ms":1700,"is_final":false}]}"#,
            2,
        );
        assert_eq!(
            texts(&b),
            vec![
                (SttKind::Final, 1, 1, "Привет как дела"),
                (SttKind::Partial, 2, 0, "Ок")
            ]
        );
    }

    #[test]
    fn soniox_finished_flag_and_flush() {
        let mut p = Parser::new(Vendor::Soniox, Stream::Me, 7_000_000);
        p.t0 = Some(T0);
        p.on_message(
            r#"{"tokens":[{"text":"hi","start_ms":0,"end_ms":200,"is_final":true}]}"#,
            1,
        );
        let f = p.flush(2);
        assert_eq!(texts(&f), vec![(SttKind::Final, 7_000_001, 1, "hi")]);
        p.on_message(r#"{"tokens":[],"finished":true}"#, 3);
        assert!(p.finished);
    }

    #[test]
    fn deepgram_interim_final_speech_final() {
        let mut p = Parser::new(Vendor::Deepgram, Stream::Me, 0);
        p.t0 = Some(T0);
        let m = |fin: bool, sf: bool, t: &str, s: f64, d: f64| {
            format!(
                r#"{{"type":"Results","is_final":{fin},"speech_final":{sf},"start":{s},"duration":{d},"channel":{{"alternatives":[{{"transcript":"{t}"}}]}}}}"#
            )
        };
        let a = p.on_message(&m(false, false, "hello wor", 0.0, 0.8), 1);
        assert_eq!(texts(&a), vec![(SttKind::Partial, 1, 0, "hello wor")]);
        let b = p.on_message(&m(true, false, "Hello world.", 0.0, 1.2), 2);
        assert_eq!(texts(&b), vec![(SttKind::Partial, 1, 1, "Hello world.")]);
        let c = p.on_message(&m(false, false, "how are", 1.3, 0.5), 3);
        assert_eq!(texts(&c), vec![(SttKind::Partial, 1, 2, "Hello world. how are")]);
        let d = p.on_message(&m(true, true, "How are you?", 1.3, 1.0), 4);
        assert_eq!(
            texts(&d),
            vec![
                (SttKind::Partial, 1, 3, "Hello world. How are you?"),
                (SttKind::Final, 1, 4, "Hello world. How are you?")
            ]
        );
        assert_eq!(d[1].t_audio_end_ns, T0 + 2_300_000_000);
        // next utterance gets a fresh id and rev
        let e = p.on_message(&m(false, false, "ok", 3.0, 0.2), 5);
        assert_eq!(texts(&e), vec![(SttKind::Partial, 2, 0, "ok")]);
    }

    #[test]
    fn keys_never_appear_in_urls_or_descriptors() {
        assert!(!deepgram_url(&["ru".into(), "en".into()]).contains("key"));
        assert!(deepgram_url(&["ru".into(), "en".into()]).contains("language=multi"));
        assert!(deepgram_url(&["ru".into()]).contains("language=ru"));
    }
}
