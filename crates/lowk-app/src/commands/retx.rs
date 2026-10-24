//! «Перераспознать запись» (M3): offline re-transcription of a call's saved audio. Glue over
//! `m2::Services`. Events: `retx://event` (`RetxEvent`). Local-only folders never use the cloud engine.
use crate::state::Shared;
use lowk_types::*;
use tauri::State;

/// Saved audio present?, running / paused, progress, current transcript revision and the older ones.
#[tauri::command(async)]
#[specta::specta]
pub fn retx_status(st: State<'_, Shared>, call: CallId) -> Result<RetxStatus> {
    st.svc()?.retx_status(&call)
}

/// Starts the pass in the background (progress arrives as `retx://event`). The live transcript is kept
/// as a revision; the call text is replaced only when the pass completes.
#[tauri::command(async)]
#[specta::specta]
pub fn retx_start(st: State<'_, Shared>, call: CallId) -> Result<()> {
    st.svc()?.retx_start(call, false)
}

/// Cancels a running pass; the call stays exactly as it was.
#[tauri::command(async)]
#[specta::specta]
pub fn retx_cancel(st: State<'_, Shared>, call: CallId) -> Result<()> {
    st.svc()?.retx_cancel(&call);
    Ok(())
}

/// Brings an older transcript revision (the live one) back as the current text.
#[tauri::command(async)]
#[specta::specta]
pub fn retx_restore(st: State<'_, Shared>, call: CallId, rev: u32) -> Result<()> {
    st.svc()?.retx_restore(&call, rev)
}
