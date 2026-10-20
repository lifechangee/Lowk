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
use lowk_types::fakes::NoopRetriever;
use lowk_types::*;
use parking_lot::RwLock;
use std::sync::{Arc, Weak};
use tauri::{App, Manager};

type BoxErr = Box<dyn std::error::Error>;

/// Session bus -> visibility. Dispatched to the driver thread so the session
/// thread never waits for the main thread.
struct PhaseWatcher(Weak<AppState>, parking_lot::Mutex<SessionPhase>);
impl PhaseWatcher {
    fn new(st: Weak<AppState>) -> Self {
        Self(st, parking_lot::Mutex::new(SessionPhase::Idle))
    }
}
impl EventSink for PhaseWatcher {
    fn emit(&self, ev: AppEvent) {
        let AppEvent::Session(s) = ev else { return };
        let prev = std::mem::replace(&mut *self.1.lock(), s.phase);
        if let Some(st) = self.0.upgrade() {
            let st2 = st.clone();
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

    let mut deps = SessionDeps::new(
        main_thread.clone(),
        Arc::new(NoopRetriever),
        settings.clone() as Arc<dyn SettingsStore>,
    );
    deps.store = Arc::new(VaultCallStore {
        vault: vault.clone(),
        emitter: emitter.clone(),
    });
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
    ));
    app.manage(state.clone());
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
            stealth.set_tone_mode(s.overlay.tone_mode);
            if let Some(g) = capture::screen_grab() {
                stealth.overlay.set_selftest_capture(g);
            }
            let stealth = Arc::new(stealth);
            *state.tone.lock() = Some(stealth.spawn_tone_sampler(capture::sampler()));
            let _ = state.stealth.set(stealth);
            state.place_overlay();
        }
        Err(e) => {
            // Fail closed: no stealth object means the overlay can never be shown.
            tracing::error!(%e, "stealth unavailable; overlay stays hidden");
            emitter.emit(AppEvent::Stealth(StealthStatus::Unavailable(e.to_string())));
        }
    }

    // ---- input ----
    apply_binds(&state);
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
    }

    // ---- initial visibility, then the main window ----
    let st2 = state.clone();
    state
        .driver
        .run(move || visibility::apply(&st2, SessionPhase::Idle));
    windows::open_main(&handle);
    Ok(())
}

/// (Re)apply `settings.binds` through the input manager and cache the report.
pub fn apply_binds(st: &AppState) {
    let binds = st.settings.get().binds;
    let r = st.input.lock().set_binds(&binds);
    match r {
        Ok(rep) => {
            *st.binds_report.lock() = rep;
            *st.applied_binds.lock() = binds;
        }
        Err(e) => tracing::error!(%e, "binds not applied"),
    }
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

fn on_settings(st: &Shared, old: &Settings, new: &Settings) {
    st.emitter.broadcast(
        lowk_types::events::SETTINGS_CHANGED,
        serde_json::to_value(new).unwrap_or_default(),
    );
    if let Ok(stealth) = st.stealth() {
        if old.stealth.hide_main != new.stealth.hide_main {
            if let Err(e) = stealth.set_hide_main(new.stealth.hide_main) {
                tracing::warn!(%e, "hide_main not applied");
            }
        }
        if old.overlay.tone_mode != new.overlay.tone_mode {
            stealth.set_tone_mode(new.overlay.tone_mode);
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
