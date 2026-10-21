//! Speech-to-text: engine traits, registry, sidecar host (local Nemotron via the ggml
//! worker), cloud websocket adapters (Soniox, Deepgram), model downloads, background
//! Whisper correction and latency diagnostics. Heavy runtimes live in the out-of-process
//! worker (`workers/lowk-stt-worker`), so this crate builds and tests everywhere.
use lowk_types::*;
use std::sync::Arc;

pub mod cloud;
pub mod corrector;
pub mod diag;
pub mod local;
pub mod models;
pub mod ring;
pub mod sidecar;

pub use corrector::{CorrectionJob, CorrectionSink, Corrector, FakeCorrector, WhisperCorrector};
pub use models::{DownloadSpec, ModelStore};

#[derive(Clone, Debug)]
pub struct SttConfig {
    pub stream: Stream,
    pub langs: Vec<LangTag>,
    pub vocab_hints: Vec<String>,
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
    /// Force-final the open utterance (asynchronous: the final arrives through the sink).
    fn flush(&mut self) -> Result<()>;
    /// Blocks until the engine has delivered everything it still owes, then releases it.
    fn stop(&mut self) -> Result<()>;
    fn backlog_ms(&self) -> u32;
    /// Algorithmic look-ahead of the model (on top of `emit_lag` in word-to-screen).
    fn lookahead_ms(&self) -> u32 {
        0
    }
    /// Latency counters for the debug page.
    fn stats(&self) -> Option<StreamDiag> {
        None
    }
    /// False while the engine is still loading its model (local sidecars: seconds on a cold
    /// start). Audio pushed meanwhile is queued, not lost.
    fn is_ready(&self) -> bool {
        true
    }
}

pub trait SttEngineFactory: Send + Sync {
    fn descriptor(&self) -> EngineDescriptor;
    fn fitness(&self, hw: &HwInfo) -> Fitness;
    fn create(&self, stream: Stream) -> Result<Box<dyn SttEngine>>;
    fn assets(&self) -> Vec<DownloadSpec>;
    /// Assets present in app data (always true for cloud / fake).
    fn installed(&self) -> bool {
        self.assets().is_empty()
    }
    /// Downloads missing assets; progress per file.
    fn install(
        &self,
        _cancel: &std::sync::atomic::AtomicBool,
        _progress: models::ProgressFn<'_>,
    ) -> Result<()> {
        Ok(())
    }
    fn needs_key(&self) -> bool {
        false
    }
    fn license_url(&self) -> String {
        String::new()
    }
    /// False when the program that runs the engine cannot be found (local sidecars).
    fn runtime_available(&self) -> bool {
        true
    }
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
    pub fn factory(&self, id: &str) -> Option<&dyn SttEngineFactory> {
        self.factories
            .iter()
            .find(|f| f.descriptor().id == id)
            .map(|b| &**b)
    }
    pub fn create(&self, id: &str, stream: Stream) -> Result<Box<dyn SttEngine>> {
        self.factory(id)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail(format!("engine {id}")))?
            .create(stream)
    }

    /// Engine the hardware check picks: the first local engine when the machine can run
    /// one, otherwise the user's cloud vendor (or the first cloud engine).
    pub fn recommended(&self, rep: &HwReport, cloud_vendor: &str) -> Option<String> {
        let rec = lowk_platform::hw::recommend(rep);
        if rec.local() {
            if let Some(f) = self.factories.iter().find(|f| {
                let d = f.descriptor();
                d.kind == EngineKind::Local
                    && d.id != "fake"
                    && !matches!(f.fitness(&rep.info), Fitness::Unsupported(_))
            }) {
                return Some(f.descriptor().id);
            }
        }
        self.factories
            .iter()
            .find(|f| f.descriptor().kind == EngineKind::Cloud && f.descriptor().id == cloud_vendor)
            .or_else(|| {
                self.factories
                    .iter()
                    .find(|f| f.descriptor().kind == EngineKind::Cloud)
            })
            .map(|f| f.descriptor().id)
    }

    /// Rows for the engine picker.
    pub fn infos(&self, rep: &HwReport, cloud_vendor: &str) -> Vec<EngineInfo> {
        let rec = self.recommended(rep, cloud_vendor);
        self.factories
            .iter()
            .filter(|f| f.descriptor().id != "fake")
            .map(|f| {
                let d = f.descriptor();
                EngineInfo {
                    fitness: f.fitness(&rep.info),
                    installed: f.installed(),
                    recommended: rec.as_deref() == Some(d.id.as_str()),
                    needs_key: f.needs_key(),
                    license_url: f.license_url(),
                    runtime_missing: !f.runtime_available(),
                    extra_mb: 0,
                    descriptor: d,
                }
            })
            .collect()
    }

    pub fn install(
        &self,
        id: &str,
        cancel: &std::sync::atomic::AtomicBool,
        progress: models::ProgressFn<'_>,
    ) -> Result<()> {
        self.factory(id)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail(format!("engine {id}")))?
            .install(cancel, progress)
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

    fn rep(os: Os, chip: &str, ram: f32, gen: Option<u8>) -> HwReport {
        HwReport {
            info: HwInfo {
                os,
                os_version: String::new(),
                arch: "aarch64".into(),
                chip: chip.into(),
                ram_gb: ram,
                gpus: vec![],
                cpu_features: vec![],
                free_disk_gb: 100.0,
                on_battery: false,
            },
            physical_cores: 10,
            logical_cores: 10,
            apple_silicon_gen: gen,
        }
    }

    struct Dummy(EngineKind, &'static str);
    impl SttEngineFactory for Dummy {
        fn descriptor(&self) -> EngineDescriptor {
            EngineDescriptor {
                id: self.1.into(),
                kind: self.0,
                ..fake_descriptor()
            }
        }
        fn fitness(&self, _: &HwInfo) -> Fitness {
            Fitness::Possible
        }
        fn create(&self, _: Stream) -> Result<Box<dyn SttEngine>> {
            Err(AppError::new(ErrorCode::NotImplemented))
        }
        fn assets(&self) -> Vec<DownloadSpec> {
            vec![]
        }
    }

    #[test]
    fn recommendation_follows_the_hardware_check() {
        let mut r = EngineRegistry::default();
        r.register(Box::new(FakeEngineFactory));
        r.register(Box::new(Dummy(EngineKind::Cloud, "soniox")));
        r.register(Box::new(Dummy(EngineKind::Cloud, "deepgram")));
        r.register(Box::new(Dummy(EngineKind::Local, "nemotron-local")));
        let m4 = rep(Os::Macos, "Apple M4", 24.0, Some(4));
        assert_eq!(r.recommended(&m4, "soniox").as_deref(), Some("nemotron-local"));
        let weak = rep(Os::Macos, "Intel", 16.0, None);
        assert_eq!(r.recommended(&weak, "deepgram").as_deref(), Some("deepgram"));
        assert!(r.infos(&m4, "soniox").iter().all(|i| i.descriptor.id != "fake"));
    }
}
