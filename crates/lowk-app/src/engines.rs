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

/// Percentage of memory still free (macOS `kern.memorystatus_level`); 100 where unknown.
fn free_memory_pct() -> u32 {
    #[cfg(target_os = "macos")]
    {
        if let Ok(o) = lowk_platform::process::command("/usr/sbin/sysctl")
            .args(["-n", "kern.memorystatus_level"])
            .output()
        {
            if let Ok(v) = String::from_utf8_lossy(&o.stdout).trim().parse::<u32>() {
                return v;
            }
        }
    }
    100
}

/// Below this much free memory (percent) the background warm-up is cancelled / not started.
const WARM_MIN_FREE_PCT: u32 = 25;

/// Runs one `warm` worker at background priority; kills it when memory gets tight or the
/// app is quitting. Returns false when cancelled.
fn run_warm(exe: &std::path::Path, args: Vec<String>, stop: &AtomicBool) -> bool {
    let spec = SidecarSpec {
        exe: exe.to_path_buf(),
        args,
        nice: 19,
    };
    let Ok(mut child) = lowk_stt::sidecar::spawn_worker(&spec) else {
        return true;
    };
    lowk_stt::sidecar::drain_stderr(&mut child, "warm");
    // stdout is tiny (one line); dropping the pipe is fine
    let t0 = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return true,
            Ok(None) => {}
        }
        if stop.load(Ordering::Relaxed)
            || free_memory_pct() < WARM_MIN_FREE_PCT
            || t0.elapsed().as_secs() > 120
        {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

impl AppState {
    /// Cold-start fix: shortly after launch, load the installed local engine once at background
    /// priority so the model sits in the OS file cache and the GPU pipelines are compiled; the
    /// first Record then starts in well under a second instead of up to ~15 s. Memory-friendly
    /// (the process exits right after loading) and cancelled under memory pressure.
    pub fn spawn_engine_warmup(self: &Arc<Self>) {
        if std::env::var_os("LOWK_NO_WARMUP").is_some() {
            return;
        }
        let st = self.clone();
        let _ = std::thread::Builder::new()
            .name("lowk-warmup".into())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                let Some(id) = st.effective_engine() else { return };
                if id != local::NEMOTRON_ID || !st.engine_ready() {
                    return;
                }
                if st.session.state().phase != SessionPhase::Idle {
                    return; // a call is starting: its own workers load the model
                }
                if free_memory_pct() < WARM_MIN_FREE_PCT {
                    tracing::info!("engine warm-up skipped: memory pressure");
                    return;
                }
                let (Some(store), Some(exe)) = (ModelStore::default_location(), local::find_worker()) else {
                    return;
                };
                let stop = AtomicBool::new(false);
                let t0 = std::time::Instant::now();
                let ok = run_warm(
                    &exe,
                    vec![
                        "warm".into(),
                        "--model".into(),
                        store
                            .path(local::NEMOTRON_ID, local::NEMOTRON_FILE)
                            .to_string_lossy()
                            .into_owned(),
                        "--vad".into(),
                        store
                            .path(local::NEMOTRON_ID, local::SILERO_FILE)
                            .to_string_lossy()
                            .into_owned(),
                        "--right".into(),
                        local::LocalTuning::default().right.to_string(),
                    ],
                    &stop,
                );
                if ok && st.settings.get().stt.background_correction && local::whisper_installed(&store) {
                    run_warm(
                        &exe,
                        vec![
                            "warm".into(),
                            "--model".into(),
                            store
                                .path(local::WHISPER_ID, local::WHISPER_FILE)
                                .to_string_lossy()
                                .into_owned(),
                        ],
                        &stop,
                    );
                }
                tracing::info!(ms = t0.elapsed().as_millis() as u64, ok, "engine warm-up done");
            });
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
        // A stored id that is not a registered engine (an old onboarding wrote a placeholder)
        // means «auto», never «nothing»: otherwise every call silently had no engine.
        if !s.engine.is_empty() && self.engines.factory(&s.engine).is_some() {
            return Some(s.engine);
        }
        self.engines
            .recommended(&lowk_platform::hw::probe_report(), &s.cloud_vendor)
    }

    /// Resets a stored engine id that is not a registered engine to «auto» (empty). Old
    /// onboarding builds wrote the placeholder `whisper-large-v3-turbo`, so no call ever found an engine.
    pub fn heal_engine_setting(&self) {
        let cur = self.settings.get().stt.engine;
        if !cur.is_empty() && self.engines.factory(&cur).is_none() {
            tracing::warn!(engine = %cur, "unknown stt engine in settings; reset to auto");
            let _ = self
                .settings
                .patch(SettingsPatch(serde_json::json!({ "stt": { "engine": "" } })));
        }
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
        // One progress bar for everything this install downloads (the live model, Silero and, with
        // background correction on, the Whisper model): `total` is the grand total, `done` cumulative.
        let with_whisper = id == local::NEMOTRON_ID && self.settings.get().stt.background_correction;
        let grand_total: u64 = if id == local::NEMOTRON_ID {
            local::nemotron_assets()
                .iter()
                .chain(
                    if with_whisper {
                        local::whisper_assets()
                    } else {
                        vec![]
                    }
                    .iter(),
                )
                .map(|a| a.size_bytes)
                .sum()
        } else {
            0
        };
        let mut finished: std::collections::HashSet<String> = Default::default();
        let mut finished_bytes = 0u64;
        let mut report = |file: &str, phase: InstallPhase, done: u64, total: u64| {
            if phase == InstallPhase::Ready && finished.insert(file.to_owned()) {
                finished_bytes += total;
            }
            let (done, total) = if grand_total > 0 {
                let inflight = if finished.contains(file) { 0 } else { done };
                ((finished_bytes + inflight).min(grand_total), grand_total)
            } else {
                (done, total)
            };
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
        if res.is_ok() && with_whisper {
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
