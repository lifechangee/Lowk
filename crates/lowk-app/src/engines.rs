//! Speech engines of the app: registry, background corrector, install flow with
//! `engine://progress`, readiness check and diagnostics.
use crate::state::AppState;
use lowk_stt::cloud::{CloudFactory, Vendor};
use lowk_stt::local::{self, NemotronFactory};
use lowk_stt::sidecar::SidecarSpec;
use lowk_stt::{Corrector, EngineRegistry, ModelStore, WhisperCorrector};
use lowk_types::EventSink as _;
use lowk_types::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub fn registry() -> EngineRegistry {
    let mut reg = EngineRegistry::default();
    if let Some(store) = ModelStore::default_location() {
        reg.register(Box::new(NemotronFactory::new(store)));
    }
    reg.register(Box::new(CloudFactory(Vendor::Soniox)));
    reg.register(Box::new(CloudFactory(Vendor::Deepgram)));
    reg
}

/// Whisper large-v3-turbo corrector, built on first `start` so a model installed after
/// the app launched is picked up; inert while the worker or the model is missing.
#[derive(Default)]
pub struct LazyCorrector(parking_lot::Mutex<Option<Arc<dyn Corrector>>>);

fn build_corrector() -> Option<Arc<dyn Corrector>> {
    let store = ModelStore::default_location()?;
    let exe = local::find_worker()?;
    if !local::whisper_installed(&store) {
        return None;
    }
    let model = store.path(local::WHISPER_ID, local::WHISPER_FILE);
    Some(Arc::new(WhisperCorrector::new(SidecarSpec {
        exe,
        args: vec![
            "correct".into(),
            "--model".into(),
            model.to_string_lossy().into_owned(),
            "--threads".into(),
            "2".into(),
        ],
        nice: 15,
    })))
}

impl Corrector for LazyCorrector {
    fn start(&self) -> Result<()> {
        let mut g = self.0.lock();
        if g.is_none() {
            *g = build_corrector();
        }
        match g.as_ref() {
            Some(c) => c.start(),
            None => Err(AppError::new(ErrorCode::NotFound).with_detail("correction model")),
        }
    }
    fn submit(
        &self,
        job: lowk_stt::CorrectionJob,
        sink: Arc<dyn lowk_stt::CorrectionSink>,
        backlog: u32,
    ) -> bool {
        let c = self.0.lock().clone();
        c.is_some_and(|c| c.submit(job, sink, backlog))
    }
    fn pending(&self) -> usize {
        self.0.lock().as_ref().map_or(0, |c| c.pending())
    }
    fn stop(&self, wait: std::time::Duration) {
        let c = self.0.lock().clone();
        if let Some(c) = c {
            c.stop(wait);
        }
    }
    fn stats(&self) -> (u32, u32, Percentiles) {
        self.0
            .lock()
            .as_ref()
            .map_or((0, 0, Percentiles::default()), |c| c.stats())
    }
}

/// `AudioBackend` over the boxed per-OS backend (the session wants a sized `Mutex<dyn _>`).
pub struct DynBackend(pub Box<dyn lowk_audio::AudioBackend>);
impl lowk_audio::AudioBackend for DynBackend {
    fn list_apps(&self) -> Result<Vec<AudioApp>> {
        self.0.list_apps()
    }
    fn list_mics(&self) -> Result<Vec<DeviceInfo>> {
        self.0.list_mics()
    }
    fn start(
        &mut self,
        cfg: &lowk_audio::CaptureConfig,
        sink: lowk_audio::FrameSink,
        ev: lowk_audio::AudioEventSink,
    ) -> Result<Box<dyn lowk_audio::CaptureHandle>> {
        self.0.start(cfg, sink, ev)
    }
}

impl AppState {
    /// The engine a new call would use: the user's choice, else the hardware check's pick.
    pub fn effective_engine(&self) -> Option<String> {
        let s = self.settings.get().stt;
        if !s.engine.is_empty() {
            return Some(s.engine);
        }
        self.engines
            .recommended(&lowk_platform::hw::probe_report(), &s.cloud_vendor)
    }

    /// True when a real call can start now (assets present, worker found / key available).
    pub fn engine_ready(&self) -> bool {
        let Some(id) = self.effective_engine() else {
            return false;
        };
        let Some(f) = self.engines.factory(&id) else {
            return false;
        };
        if f.needs_key() {
            let v = if id == lowk_stt::cloud::DEEPGRAM_ID {
                Vendor::Deepgram
            } else {
                Vendor::Soniox
            };
            return lowk_stt::cloud::resolve_key(v).is_ok();
        }
        f.installed() && local::find_worker().is_some()
    }

    /// Downloads an engine's assets (and the Whisper correction model for local engines),
    /// emitting `engine://progress`. Blocks; run it off the UI thread.
    pub fn install_engine(&self, id: &str) -> Result<()> {
        let cancel = Arc::new(AtomicBool::new(false));
        *self.install_cancel.lock() = Some(cancel.clone());
        let em = self.emitter.clone();
        let engine = id.to_owned();
        let mut report = |file: &str, phase: InstallPhase, done: u64, total: u64| {
            em.emit(AppEvent::EngineProgress(EngineProgress {
                engine: engine.clone(),
                file: file.to_owned(),
                phase,
                done_bytes: done,
                total_bytes: total,
                message: None,
            }))
        };
        let mut res = self.engines.install(id, &cancel, &mut report);
        if res.is_ok() && id == local::NEMOTRON_ID && self.settings.get().stt.background_correction {
            if let Some(store) = ModelStore::default_location() {
                res = local::install_whisper(&store, &cancel, &mut report);
            }
        }
        *self.install_cancel.lock() = None;
        if let Err(e) = &res {
            self.emitter.emit(AppEvent::EngineProgress(EngineProgress {
                engine: id.to_owned(),
                file: String::new(),
                phase: InstallPhase::Failed,
                done_bytes: 0,
                total_bytes: 0,
                // the UI maps the machine-readable reason (checksum / cancelled / network) to text
                message: Some(format!(
                    "{e} {}",
                    e.params.get("reason").map_or("", String::as_str)
                )),
            }));
        }
        res
    }

    pub fn cancel_install(&self) {
        if let Some(c) = self.install_cancel.lock().as_ref() {
            c.store(true, Ordering::SeqCst);
        }
    }
}
