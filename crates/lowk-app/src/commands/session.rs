//! session, sources, engines, demo, and the thin LLM/capture entry points the
//! overlay calls (the real LLM and screenshots arrive in M3).
use crate::state::{AppState, Shared};
use lowk_types::*;
use std::sync::Arc;
use tauri::State;

/// A call is real when an engine is ready (assets installed / key present); otherwise the
/// scripted demo runs, so the app stays usable before onboarding installs an engine.
/// `LOWK_DEMO=1` forces the demo (QA, screenshots).
fn effective(st: &AppState, mut o: StartOpts) -> StartOpts {
    if std::env::var_os("LOWK_DEMO").is_some() || !st.engine_ready() {
        o.demo = true;
    }
    o
}

fn start(st: &AppState, o: StartOpts) -> Result<SessionState> {
    if st.vault.read().is_none() {
        st.open_vault(None)?;
    }
    let o = effective(st, o);
    st.session.start(o)?;
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
            },
        )
        .map(|_| ()),
        SessionPhase::Recording | SessionPhase::Paused => st.session.stop(),
        _ => Ok(()),
    }
}

#[tauri::command(async)]
#[specta::specta]
pub fn session_start(st: State<'_, Shared>, opts: StartOpts) -> Result<SessionState> {
    start(&st, opts)
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
    start(
        st,
        StartOpts {
            title: None,
            folder: None,
            system: None,
            mic: None,
            demo: true,
        },
    )
}

/// The scripted RU/EN demo call (also what the record bind plays in M0).
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
    st.engines
        .infos(&lowk_platform::hw::probe_report(), &st.settings.get().stt.cloud_vendor)
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
pub fn llm_cancel() -> Result<()> {
    Err(AppError::not_implemented())
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

#[tauri::command(async)]
#[specta::specta]
pub fn capture_screenshot() -> Result<()> {
    Err(AppError::not_implemented())
}

#[tauri::command(async)]
#[specta::specta]
pub fn queue_clear() -> Result<()> {
    Ok(())
}
