//! session, sources, engines, demo, and the thin LLM/capture entry points the
//! overlay calls (the real LLM and screenshots arrive in M3).
use crate::state::{AppState, Shared};
use lowk_types::*;
use std::sync::Arc;
use tauri::State;

/// The scripted demo exists only in dev/QA builds: the `qa` feature (which also falls back to it
/// while no engine is installed, so the harness needs no model) or a debug build with
/// `LOWK_DEMO=1`. A shipped build never fakes a call (M1 fix report, C).
pub(crate) fn demo_allowed() -> bool {
    cfg!(feature = "qa") || (cfg!(debug_assertions) && std::env::var_os("LOWK_DEMO").is_some())
}

/// What the owner needs to see when a real call cannot start: the engine a call would use,
/// whether it is a download (size) or a cloud key. Sent as `engine://needed` to the main window.
#[derive(serde::Serialize)]
struct EngineNeeded {
    engine: String,
    kind: &'static str,
    size_mb: u32,
}

fn announce_engine_needed(st: &AppState) {
    let id = st.effective_engine().unwrap_or_default();
    tracing::info!(engine = %id, "record refused: no speech engine ready; showing the download / cloud prompt");
    let (kind, size_mb) = st
        .engines
        .available()
        .into_iter()
        .find(|d| d.id == id)
        .map_or(("none", 0), |d| {
            (
                if d.kind == EngineKind::Local {
                    "local"
                } else {
                    "cloud"
                },
                d.size_mb,
            )
        });
    st.emitter.broadcast(
        "engine://needed",
        serde_json::to_value(EngineNeeded {
            engine: id,
            kind,
            size_mb,
        })
        .unwrap_or_default(),
    );
    crate::windows::open_main(&st.app);
}

/// A call is real or does not start: no engine ready -> `EngineNotReady` and the download /
/// cloud prompt. The demo is reachable only where `demo_allowed()`.
fn effective(st: &AppState, mut o: StartOpts) -> Result<StartOpts> {
    if demo_allowed() {
        if std::env::var_os("LOWK_DEMO").is_some() || !st.engine_ready() {
            o.demo = true;
        }
        return Ok(o);
    }
    o.demo = false;
    if !st.engine_ready() {
        announce_engine_needed(st);
        return Err(AppError::new(ErrorCode::EngineNotReady));
    }
    Ok(o)
}

fn start(st: &AppState, o: StartOpts) -> Result<SessionState> {
    if st.vault.read().is_none() {
        st.open_vault(None)?;
    }
    st.heal_engine_setting();
    let o = effective(st, o)?;
    let folder = o.folder.clone();
    st.session.start(o)?;
    // the first Ask comes within seconds: connect the LLM provider and wake the embedder now
    st.spawn_ask_warmup(folder);
    Ok(st.session.state())
}

pub fn toggle_record(st: &Arc<AppState>) -> Result<()> {
    match st.session.state().phase {
        SessionPhase::Idle => start(
            st,
            StartOpts {
                title: None,
                folder: None,
                system: None,
                mic: None,
                demo: false,
                languages: None,
            },
        )
        .map(|_| ()),
        SessionPhase::Recording | SessionPhase::Paused => st.session.stop(),
        _ => Ok(()),
    }
}

/// The microphone bind: flips the mic of the running recording (idle: `SessionIdle`, which the bind handler ignores).
pub fn toggle_mic(st: &Arc<AppState>) -> Result<()> {
    let on = !st.session.state().mic_on;
    st.session.set_mic_enabled(on)
}

/// Overlay button / main-window control: microphone on or off while recording, without stopping it.
#[tauri::command(async)]
#[specta::specta]
pub fn session_set_mic(st: State<'_, Shared>, on: bool) -> Result<SessionState> {
    st.session.set_mic_enabled(on)?;
    Ok(st.session.state())
}

/// Microphone mode of the running recording: on, off or auto (only while the call app holds the microphone).
#[tauri::command(async)]
#[specta::specta]
pub fn session_set_mic_mode(st: State<'_, Shared>, mode: MicMode) -> Result<SessionState> {
    st.session.set_mic_mode(mode)?;
    Ok(st.session.state())
}

#[tauri::command(async)]
#[specta::specta]
pub fn session_start(st: State<'_, Shared>, opts: StartOpts) -> Result<SessionState> {
    let langs = opts.languages.clone();
    let state = start(&st, opts)?;
    // The per-call choice is remembered for the next call (and for a call started by the record bind).
    if let Some(l) = langs.filter(|l| !l.is_empty()) {
        let l = lowk_types::langpolicy::normalize(&l);
        if l != st.settings.get().stt.last_languages {
            let _ = st.settings.patch(SettingsPatch(
                serde_json::json!({ "stt": { "last_languages": l } }),
            ));
        }
    }
    Ok(state)
}

#[tauri::command(async)]
#[specta::specta]
pub fn session_stop(st: State<'_, Shared>) -> Result<()> {
    st.session.stop()
}

#[tauri::command(async)]
#[specta::specta]
pub fn session_pause(st: State<'_, Shared>) -> Result<()> {
    st.session.pause()
}

#[tauri::command(async)]
#[specta::specta]
pub fn session_resume(st: State<'_, Shared>) -> Result<()> {
    st.session.resume()
}

#[tauri::command(async)]
#[specta::specta]
pub fn session_state(st: State<'_, Shared>) -> SessionState {
    st.session.state()
}

#[tauri::command(async)]
#[specta::specta]
pub fn session_snapshot(st: State<'_, Shared>) -> SessionSnapshot {
    st.session.snapshot()
}

pub fn demo_start_inner(st: &AppState) -> Result<SessionState> {
    if !demo_allowed() {
        return Err(AppError::new(ErrorCode::Unsupported).with_detail("demo is a dev/QA feature"));
    }
    start(
        st,
        StartOpts {
            title: None,
            folder: None,
            system: None,
            mic: None,
            demo: true,
            languages: None,
        },
    )
}

/// The scripted RU/EN demo call (dev/QA builds only).
#[tauri::command(async)]
#[specta::specta]
pub fn demo_start(st: State<'_, Shared>) -> Result<SessionState> {
    demo_start_inner(&st)
}

/// Source picker data: the whole system plus apps that have an audio process right now
/// (producing ones first). Cheap; the UI may poll at 1 Hz.
#[tauri::command(async)]
#[specta::specta]
pub async fn source_list() -> Result<Vec<SourceInfo>> {
    blocking(lowk_audio::list_sources).await
}

/// Installed apps (with icons), so a call app can be picked before it starts playing.
#[tauri::command(async)]
#[specta::specta]
pub async fn source_list_installed() -> Result<Vec<SourceInfo>> {
    blocking(lowk_audio::list_installed_sources).await
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::new(ErrorCode::Unknown).with_detail(e.to_string()))?
}

/// Static descriptors of the registered engines.
#[tauri::command(async)]
#[specta::specta]
pub fn engine_list(st: State<'_, Shared>) -> Vec<EngineDescriptor> {
    st.engines.available()
}

/// Engine picker rows: fitness for this machine, installed, recommended, needs a key.
#[tauri::command(async)]
#[specta::specta]
pub fn engine_status(st: State<'_, Shared>) -> Vec<EngineInfo> {
    let stt = st.settings.get().stt;
    let mut infos = st
        .engines
        .infos(&lowk_platform::hw::probe_report(), &stt.cloud_vendor);
    // The live model comes with the Whisper correction model while background correction is on:
    // the install card must show the real total, not just the first download.
    if stt.background_correction {
        if let Some(store) = lowk_stt::ModelStore::default_location() {
            if !lowk_stt::local::whisper_installed(&store) {
                let mb: u32 = lowk_stt::local::whisper_assets()
                    .iter()
                    .map(|a| a.size_mb())
                    .sum();
                for i in &mut infos {
                    if i.descriptor.id == lowk_stt::local::NEMOTRON_ID {
                        i.extra_mb = mb;
                    }
                }
            }
        }
    }
    infos
}

/// Downloads an engine's models into app data (SHA-256 checked); progress arrives as
/// `engine://progress`. Returns when the engine is ready or the download failed / was cancelled.
#[tauri::command(async)]
#[specta::specta]
pub fn engine_install(st: State<'_, Shared>, id: String) -> Result<()> {
    st.install_engine(&id)
}

#[tauri::command(async)]
#[specta::specta]
pub fn engine_install_cancel(st: State<'_, Shared>) {
    st.cancel_install();
}

/// Latency percentiles per stream (debug page).
#[tauri::command(async)]
#[specta::specta]
pub fn stt_diagnostics(st: State<'_, Shared>) -> Option<SttDiagnostics> {
    st.session.stt_diagnostics()
}

// ---- overlay entry points (LLM / screenshots are M3) ------------------------

#[tauri::command(async)]
#[specta::specta]
pub fn llm_ask(st: State<'_, Shared>, typed_text: Option<String>) -> Result<QaId> {
    let template_id = st.template.lock().clone();
    st.session.ask(AskRequest {
        template_id,
        typed_text,
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn llm_cancel(st: State<'_, Shared>) -> Result<()> {
    st.session.cancel_ask();
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn llm_set_template(st: State<'_, Shared>, id: String) -> Result<()> {
    if !st.settings.get().templates.iter().any(|t| t.id == id) {
        return Err(AppError::new(ErrorCode::NotFound).with_detail("template"));
    }
    *st.template.lock() = id;
    Ok(())
}
