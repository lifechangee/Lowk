//! session, sources, engines, demo, and the thin LLM/capture entry points the
//! overlay calls (the real LLM and screenshots arrive in M3).
use crate::state::{AppState, Shared};
use lowk_types::*;
use std::sync::Arc;
use tauri::State;

/// M0 has no audio capture yet (`lowk-audio` is a trait + no-op backend), so
/// every session runs the scripted demo feed. M1 drops this.
fn effective(mut o: StartOpts) -> StartOpts {
    o.demo = true;
    o
}

fn start(st: &AppState, o: StartOpts) -> Result<SessionState> {
    if st.vault.read().is_none() {
        st.open_vault(None)?;
    }
    st.session.start(effective(o))?;
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
                demo: true,
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

/// Apps that currently produce audio (empty until `lowk-audio` lands in M1).
#[tauri::command(async)]
#[specta::specta]
pub fn source_list() -> Result<Vec<AudioApp>> {
    use lowk_audio::AudioBackend;
    lowk_audio::NoopAudioBackend.list_apps()
}

#[tauri::command(async)]
#[specta::specta]
pub fn engine_list(st: State<'_, Shared>) -> Vec<EngineDescriptor> {
    let _ = &st;
    let mut reg = lowk_stt::EngineRegistry::default();
    reg.register(Box::new(lowk_stt::FakeEngineFactory));
    reg.available()
}

#[tauri::command(async)]
#[specta::specta]
pub fn engine_install(id: String) -> Result<()> {
    let _ = id;
    Err(AppError::not_implemented())
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
