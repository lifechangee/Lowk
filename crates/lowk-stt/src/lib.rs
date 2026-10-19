//! Speech-to-text: engine traits, registry, VAD, post-processing, sidecars.
//! M0 stub: traits plus `FakeEngine`. Cargo features (`whisper`, `ort-nemotron`,
//! `cloud-soniox`, `cloud-deepgram`, `sidecar`) are added by the owner.
use lowk_types::*;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct SttConfig {
    pub stream: Stream,
    pub langs: Vec<LangTag>,
    pub vocab_hints: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DownloadSpec {
    pub url: String,
    pub sha256: String,
    pub size_mb: u32,
    pub license: String,
}

#[derive(Clone, Debug)]
pub struct BenchResult {
    pub rtf: f32,
    pub first_partial_ms: u32,
    pub p95_final_ms: u32,
}

pub trait SttEngine: Send {
    fn descriptor(&self) -> &EngineDescriptor;
    fn start(&mut self, cfg: &SttConfig, sink: Arc<dyn SttSink>) -> Result<()>;
    /// Non-blocking; the engine owns its queue/thread/socket.
    fn push(&mut self, frame: &Frame16k) -> Result<()>;
    /// Force-final the open utterance.
    fn flush(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn backlog_ms(&self) -> u32;
}

pub trait SttEngineFactory: Send + Sync {
    fn descriptor(&self) -> EngineDescriptor;
    fn fitness(&self, hw: &HwInfo) -> Fitness;
    fn create(&self, stream: Stream) -> Result<Box<dyn SttEngine>>;
    fn assets(&self) -> Vec<DownloadSpec>;
}

#[derive(Default)]
pub struct EngineRegistry {
    factories: Vec<Box<dyn SttEngineFactory>>,
}
impl EngineRegistry {
    pub fn register(&mut self, f: Box<dyn SttEngineFactory>) {
        self.factories.push(f);
    }
    pub fn available(&self) -> Vec<EngineDescriptor> {
        self.factories.iter().map(|f| f.descriptor()).collect()
    }
    pub fn create(&self, id: &str, stream: Stream) -> Result<Box<dyn SttEngine>> {
        self.factories
            .iter()
            .find(|f| f.descriptor().id == id)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound))?
            .create(stream)
    }
}

fn fake_descriptor() -> EngineDescriptor {
    EngineDescriptor {
        id: "fake".into(),
        name: "Fake engine".into(),
        kind: EngineKind::Local,
        languages: vec!["ru".into(), "en".into()],
        streaming: StreamingMode::Native,
        size_mb: 0,
        license: "AGPL-3.0-or-later".into(),
    }
}

/// Emits a `Partial` every [`FAKE_PARTIAL_EVERY`] pushed frames (rev 0, 1, ...)
/// and one `Final` (next rev) on `flush`; for contract tests.
/// Frames between two partials of [`FakeEngine`].
pub const FAKE_PARTIAL_EVERY: u64 = 10;

pub struct FakeEngine {
    desc: EngineDescriptor,
    stream: Stream,
    sink: Option<Arc<dyn SttSink>>,
    frames: u64,
    utt: UttId,
    rev: u32,
    t_start: MonoNs,
}
impl FakeEngine {
    pub fn new(stream: Stream) -> Self {
        Self {
            desc: fake_descriptor(),
            stream,
            sink: None,
            frames: 0,
            utt: 0,
            rev: 0,
            t_start: 0,
        }
    }
}
impl SttEngine for FakeEngine {
    fn descriptor(&self) -> &EngineDescriptor {
        &self.desc
    }
    fn start(&mut self, _cfg: &SttConfig, sink: Arc<dyn SttSink>) -> Result<()> {
        self.sink = Some(sink);
        Ok(())
    }
    fn push(&mut self, frame: &Frame16k) -> Result<()> {
        if self.frames == 0 {
            self.utt += 1;
            self.rev = 0;
            self.t_start = frame.t_mono_ns;
        }
        self.frames += 1;
        if self.frames % FAKE_PARTIAL_EVERY == 0 {
            if let Some(s) = &self.sink {
                s.emit(SttEvent {
                    stream: self.stream,
                    utt: self.utt,
                    rev: self.rev,
                    kind: SttKind::Partial,
                    text: format!("fake utterance ({} frames)", self.frames),
                    lang: None,
                    t_start_ns: self.t_start,
                    t_audio_end_ns: frame.t_mono_ns,
                    t_emit_ns: now_mono_ns(),
                });
                self.rev += 1;
            }
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        if self.frames == 0 {
            return Ok(());
        }
        if let Some(s) = &self.sink {
            let now = now_mono_ns();
            s.emit(SttEvent {
                stream: self.stream,
                utt: self.utt,
                rev: self.rev,
                kind: SttKind::Final,
                text: format!("fake utterance ({} frames)", self.frames),
                lang: None,
                t_start_ns: self.t_start,
                t_audio_end_ns: now.max(self.t_start),
                t_emit_ns: now,
            });
        }
        self.frames = 0;
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        self.sink = None;
        Ok(())
    }
    fn backlog_ms(&self) -> u32 {
        0
    }
}

/// Factory for [`FakeEngine`] (id `fake`); lets sessions run before real
/// backends exist.
#[derive(Default)]
pub struct FakeEngineFactory;
impl SttEngineFactory for FakeEngineFactory {
    fn descriptor(&self) -> EngineDescriptor {
        fake_descriptor()
    }
    fn fitness(&self, _hw: &HwInfo) -> Fitness {
        Fitness::Possible
    }
    fn create(&self, stream: Stream) -> Result<Box<dyn SttEngine>> {
        Ok(Box::new(FakeEngine::new(stream)))
    }
    fn assets(&self) -> Vec<DownloadSpec> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::fakes::arc_sink;

    #[test]
    fn fake_engine_emits_final_on_flush() {
        let sink = arc_sink();
        let mut e = FakeEngine::new(Stream::Me);
        e.start(
            &SttConfig {
                stream: Stream::Me,
                langs: vec![],
                vocab_hints: vec![],
            },
            sink.clone(),
        )
        .unwrap();
        e.push(&Frame16k {
            stream: Stream::Me,
            t_mono_ns: 0,
            pcm: vec![0.0; 320],
        })
        .unwrap();
        e.flush().unwrap();
        assert_eq!(sink.events.lock().unwrap().len(), 1);
    }

    #[test]
    fn fake_engine_partials_then_final_with_monotonic_revs() {
        let sink = arc_sink();
        let mut e = FakeEngine::new(Stream::System);
        let cfg = SttConfig {
            stream: Stream::System,
            langs: vec![],
            vocab_hints: vec![],
        };
        e.start(&cfg, sink.clone()).unwrap();
        let f = Frame16k {
            stream: Stream::System,
            t_mono_ns: 5,
            pcm: vec![0.0; 320],
        };
        for _ in 0..25 {
            e.push(&f).unwrap();
        }
        e.flush().unwrap();
        e.flush().unwrap(); // nothing open: no extra event
        let evs = sink.events.lock().unwrap();
        let kinds: Vec<_> = evs.iter().map(|x| (x.utt, x.rev, x.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (1, 0, SttKind::Partial),
                (1, 1, SttKind::Partial),
                (1, 2, SttKind::Final)
            ]
        );
    }
}
