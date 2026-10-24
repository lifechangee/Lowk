//! Application assembly (Tauri `setup`): settings, session, input, windows, stealth.
use crate::callstore::VaultCallStore;
use crate::combo::{self, TauriCombo};
use crate::emitter::UiEmitter;
use crate::mainthread::TauriMainThread;
use crate::state::{data_dir, AppState, Shared};
use crate::{actions, capture, visibility, windows};
use lowk_input::InputManager;
use lowk_session::{Session, SessionDeps};
use lowk_stealth::{create_stealth, RawWindows, StealthConfig};
use lowk_store::FileSettingsStore;
use lowk_types::*;
use parking_lot::RwLock;
use std::sync::{Arc, Weak};
use tauri::{App, Manager};

type BoxErr = Box<dyn std::error::Error>;

/// Session bus -> visibility. Dispatched to the driver thread so the session
/// thread never waits for the main thread.
struct PhaseWatcher(Weak<AppState>, parking_lot::Mutex<(SessionPhase, Option<CallId>)>);
impl PhaseWatcher {
    fn new(st: Weak<AppState>) -> Self {
        Self(st, parking_lot::Mutex::new((SessionPhase::Idle, None)))
    }
}
impl EventSink for PhaseWatcher {
    fn emit(&self, ev: AppEvent) {
        let AppEvent::Session(s) = ev else { return };
        let (prev, call) = {
            let mut g = self.1.lock();
            let prev = g.0;
            g.0 = s.phase;
            if s.call.is_some() {
                g.1 = s.call;
            }
            (prev, g.1)
        };
        if let Some(st) = self.0.upgrade() {
            let st2 = st.clone();
            // Indexing yields to live transcription: background embedding pauses while a call runs.
            if let Some(svc) = st.m2.read().clone() {
                svc.index.pause_embedding(!matches!(s.phase, SessionPhase::Idle));
                // a running re-transcription waits while a recording runs
                svc.retx_set_recording(!matches!(s.phase, SessionPhase::Idle));
                // «Перераспознавать после записи»: the call that just ended (audio kept, setting / folder preset on)
                if s.phase == SessionPhase::Idle && prev != SessionPhase::Idle && !s.demo {
                    if let Some(c) = call {
                        svc.retx_after_recording(c);
                    }
                }
            }
            // F8: a recording that just ended (stop button, bind or end of call) brings the
            // main window to the front on the library home.
            let ended = s.phase == SessionPhase::Idle && prev != SessionPhase::Idle;
            st.driver.run(move || {
                visibility::apply(&st2, s.phase);
                if ended {
                    visibility::session_ended(&st2);
                }
            });
        }
    }
}

pub fn setup(app: &mut App) -> std::result::Result<(), BoxErr> {
    let handle = app.handle().clone();
    let main_thread: Arc<dyn MainThread> = Arc::new(TauriMainThread::new(handle.clone()));

    let settings = Arc::new(FileSettingsStore::open(&data_dir()?)?);
    if let Some(w) = settings.load_warning() {
        tracing::warn!("settings.toml was unreadable and reset: {w}");
    }
    let emitter = UiEmitter::new(handle.clone());
    let vault = Arc::new(RwLock::new(None));
    migrate_settings(&settings);

    // M2: documents, index, embedder, LLM routing. The retriever and the hint generator are the
    // real ones; there is no stub path (no key means a clear error in the overlay).
    let index_slot: Arc<RwLock<Option<lowk_index::Index>>> = Arc::new(RwLock::new(None));
    let m2_env = crate::m2::Env {
        settings: settings.clone() as Arc<dyn SettingsStore>,
        sink: emitter.clone(),
        keys: Arc::new(crate::m2::SystemKeys),
        models: lowk_index::embedder::models::ModelStore::default_location().unwrap_or_else(|| {
            lowk_index::embedder::models::ModelStore::new(data_dir().unwrap_or_default().join("models"))
        }),
        embed_worker: None,
    };
    let mut deps = SessionDeps::new(
        main_thread.clone(),
        Arc::new(crate::m2::DynRetriever(index_slot.clone())),
        settings.clone() as Arc<dyn SettingsStore>,
    );
    deps.hints = Arc::new(crate::m2::hints::LlmHints::new(m2_env.clone(), vault.clone()));
    deps.store = Arc::new(VaultCallStore {
        vault: vault.clone(),
        emitter: emitter.clone(),
    });
    // «Record microphone» preset of the folder a new call starts in (`_folder.md`); unreadable = inherit.
    deps.folder_mic = {
        let vault = vault.clone();
        Arc::new(move |f: &FolderRef| match f {
            FolderRef::User(id) => vault
                .read()
                .as_ref()
                .and_then(|v| v.folder_doc(id).ok())
                .map(|d| d.mic)
                .unwrap_or_default(),
            _ => MicPreset::Inherit,
        })
    };
    deps.record_demo = true; // M0: demo calls are real calls, so the library has content
                             // M1: real capture + local/cloud STT. Demo stays the fallback while no engine is ready.
    deps.audio = Arc::new(std::sync::Mutex::new(crate::engines::DynBackend(
        lowk_audio::default_backend(),
    )));
    // QA only (feature `qa`, debug builds): replay files as the Me / System streams instead of
    // capturing, so end-to-end checks make no sound and need no permissions.
    #[cfg(feature = "qa")]
    {
        let f = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
        let (me, sys) = (f("LOWK_QA_ME_FILE"), f("LOWK_QA_SYSTEM_FILE"));
        if me.is_some() || sys.is_some() {
            deps.audio = Arc::new(std::sync::Mutex::new(lowk_audio::FileAudioSource::new(me, sys)));
        }
    }
    let engines = Arc::new(crate::engines::registry());
    deps.stt = engines.clone();
    deps.corrector = Some(Arc::new(crate::engines::LazyCorrector::default()));
    deps.default_engine = engines.recommended(
        &lowk_platform::hw::probe_report(),
        &settings.get().stt.cloud_vendor,
    );
    let session = Arc::new(Session::new(deps));
    session.add_sink(emitter.clone());
    crate::sigterm::install(handle.clone(), session.clone());

    let input = InputManager::new(Some(Box::new(TauriCombo::new(handle.clone()))));
    let bind_rx = input.events();
    if let Some(e) = input.combo_events() {
        combo::hub().set_events(e);
    }

    let state: Shared = Arc::new(AppState::new(
        handle.clone(),
        settings.clone(),
        vault,
        session.clone(),
        input,
        emitter.clone(),
        engines,
        m2_env,
        index_slot,
    ));
    app.manage(state.clone());
    {
        // every store change (commands, call recorder, vault watcher) also feeds the index
        let weak = Arc::downgrade(&state);
        emitter.set_store_tap(Box::new(move |c| {
            if let Some(st) = weak.upgrade() {
                if let Some(svc) = st.m2.read().clone() {
                    svc.on_store_change(c);
                }
            }
        }));
    }
    session.add_sink(Arc::new(PhaseWatcher::new(Arc::downgrade(&state))));

    // ---- stealth: windows are created hidden, protected before the first show ----
    let overlay = windows::build_overlay(&handle)?;
    let pill = windows::build_pill(&handle)?;
    let s = settings.get();
    let raws = RawWindows {
        overlay: Some(windows::raw_window(&overlay)?),
        main: None,
        pill: Some(windows::raw_window(&pill)?),
    };
    let tone_emitter = emitter.clone();
    match create_stealth(
        raws,
        main_thread.clone(),
        StealthConfig {
            hide_main: s.stealth.hide_main,
            verify_interval_ms: s.stealth.verify_interval_ms,
        },
        Box::new(move |t| tone_emitter.emit(AppEvent::Overlay(OverlayEvent::Tone(t)))),
    ) {
        Ok(stealth) => {
            stealth.set_event_sink(emitter.clone());
            stealth.set_tone_mode(s.overlay.effective_tone_mode());
            if let Some(g) = capture::screen_grab() {
                stealth.overlay.set_selftest_capture(g);
            }
            let stealth = Arc::new(stealth);
            // The sampler (ScreenCaptureKit / Windows capture) exists only for the adaptive scrim, which is opt-in.
            state.tone.set_enabled(s.overlay.sampler_wanted(), || {
                stealth.spawn_tone_sampler(capture::sampler())
            });
            let _ = state.stealth.set(stealth);
            state.place_overlay();
        }
        Err(e) => {
            // Fail closed: no stealth object means the overlay can never be shown.
            tracing::error!(%e, "stealth unavailable; overlay stays hidden");
            emitter.emit(AppEvent::Stealth(StealthStatus::Unavailable(e.to_string())));
        }
    }

    state.heal_engine_setting();
    log_startup_facts(&state);

    // ---- input ----
    apply_binds(&state);
    spawn_input_recheck(state.clone());
    actions::spawn(state.clone(), bind_rx);

    #[cfg(debug_assertions)]
    crate::probe::spawn(state.clone());
    #[cfg(feature = "qa")]
    crate::qa::spawn(state.clone());

    // ---- settings side effects ----
    spawn_settings_watcher(state.clone());
    state.spawn_engine_warmup();

    // ---- vault ----
    if s.onboarding.done {
        if let Err(e) = state.open_vault(None) {
            tracing::warn!(%e, "vault not opened at startup");
        }
        state.spawn_ask_warmup(None);
    }

    // ---- initial visibility, then the main window ----
    let st2 = state.clone();
    state
        .driver
        .run(move || visibility::apply(&st2, SessionPhase::Idle));
    windows::open_main(&handle);
    Ok(())
}

/// One greppable block in `lowk.log` per launch: what the bundle can actually do on this Mac.
/// `tools/qa/bundle-qa.mjs` reads it (a bundle has no other observable surface).
fn log_startup_facts(st: &AppState) {
    let perms: Vec<String> = lowk_platform::perm::ALL
        .iter()
        .map(|k| format!("{k:?}={:?}", lowk_platform::perm::status(*k)))
        .collect();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        demo_allowed = crate::commands::session::demo_allowed(),
        engine = ?st.effective_engine(),
        engine_ready = st.engine_ready(),
        worker = ?lowk_stt::local::find_worker(),
        "startup: engine"
    );
    tracing::info!(perms = %perms.join(" "), "startup: permissions");
}

/// (Re)apply `settings.binds` through the input manager, cache the report, log what is
/// degraded (bundle diagnostics: dev builds inherit the host app's permissions, bundles do not)
/// and tell the windows (`binds://report`) so the overlay hint and Settings show the truth.
pub fn apply_binds(st: &AppState) {
    let binds = st.settings.get().binds;
    let r = st.input.lock().set_binds(&binds);
    match r {
        Ok(rep) => {
            for e in &rep.entries {
                match &e.status {
                    BindStatus::Active => tracing::debug!(action = ?e.action, "bind active"),
                    other => tracing::warn!(action = ?e.action, status = ?other, "bind not fully active"),
                }
            }
            tracing::info!(
                total = rep.entries.len(),
                active = rep
                    .entries
                    .iter()
                    .filter(|e| e.status == BindStatus::Active)
                    .count(),
                "binds applied"
            );
            st.emitter
                .broadcast("binds://report", serde_json::to_value(&rep).unwrap_or_default());
            *st.binds_report.lock() = rep;
            *st.applied_binds.lock() = binds;
        }
        Err(e) => tracing::error!(%e, "binds not applied"),
    }
}

/// Hold / modifier-only / mouse binds need Input Monitoring. When the user grants it later
/// the hook must start without a manual step: poll the permission cheaply and re-apply once.
fn spawn_input_recheck(st: Shared) {
    std::thread::Builder::new()
        .name("lowk-input-recheck".into())
        .spawn(move || {
            let mut last = lowk_input::platform::permissions();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let now = lowk_input::platform::permissions();
                if now == last {
                    continue;
                }
                last = now;
                let degraded = st
                    .binds_report
                    .lock()
                    .entries
                    .iter()
                    .any(|e| matches!(e.status, BindStatus::Degraded(_)));
                if degraded {
                    tracing::info!(?now, "input permission changed; re-applying binds");
                    apply_binds(&st);
                }
            }
        })
        .expect("spawn input recheck");
}

fn spawn_settings_watcher(st: Shared) {
    let rx = st.settings.subscribe();
    std::thread::Builder::new()
        .name("lowk-settings".into())
        .spawn(move || {
            let mut last = st.settings.get();
            for new in rx {
                on_settings(&st, &last, &new);
                last = new;
            }
        })
        .expect("spawn settings watcher");
}

/// Settings written by older builds.
fn migrate_settings(settings: &lowk_store::FileSettingsStore) {
    // the unpinned slug was the default for a while (2.3 s TTFT p50 vs 1.2 s pinned)
    if settings.get().llm.model == LEGACY_LLM_MODEL {
        tracing::info!("migrating llm.model to the version-pinned default");
        let _ = settings.patch(SettingsPatch(
            serde_json::json!({ "llm": { "model": DEFAULT_LLM_MODEL } }),
        ));
    }
}

fn on_settings(st: &Shared, old: &Settings, new: &Settings) {
    st.emitter.broadcast(
        lowk_types::events::SETTINGS_CHANGED,
        serde_json::to_value(new).unwrap_or_default(),
    );
    if let Some(svc) = st.m2.read().clone() {
        svc.on_settings(old, new);
    }
    if let Ok(stealth) = st.stealth() {
        if old.stealth.hide_main != new.stealth.hide_main {
            if let Err(e) = stealth.set_hide_main(new.stealth.hide_main) {
                tracing::warn!(%e, "hide_main not applied");
            }
        }
        if old.overlay.effective_tone_mode() != new.overlay.effective_tone_mode() {
            stealth.set_tone_mode(new.overlay.effective_tone_mode());
        }
        if old.overlay.sampler_wanted() != new.overlay.sampler_wanted() {
            let on = new.overlay.sampler_wanted();
            let running = st
                .tone
                .set_enabled(on, || stealth.spawn_tone_sampler(capture::sampler()));
            tracing::info!(adaptive_scrim = on, running, "adaptive scrim sampler switched");
        }
    }
    // F12: the chosen source (onboarding / settings) and the mode the UI shows never disagree.
    if old.audio.default_source != new.audio.default_source {
        let mode = CaptureSource::of_target(new.audio.default_source.as_ref());
        if new.capture.default_source != mode {
            let _ = st.settings.patch(SettingsPatch(
                serde_json::json!({ "capture": { "default_source": mode } }),
            ));
        }
    }
    if new.binds != *st.applied_binds.lock() {
        apply_binds(st);
    }
    // G2: choosing another edge re-docks the pill (forget the dragged position).
    if old.overlay.pill_edge != new.overlay.pill_edge && !new.overlay.pill_positions.is_empty() {
        let _ = st.settings.patch(SettingsPatch(
            serde_json::json!({ "overlay": { "pill_positions": {} } }),
        ));
    }
    if old.overlay.visibility != new.overlay.visibility || old.overlay.pill_edge != new.overlay.pill_edge {
        let st2 = st.clone();
        let phase = st.session.state().phase;
        st.driver.run(move || visibility::apply(&st2, phase));
    }
}
