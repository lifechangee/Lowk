//! G7: screenshot queue. `capture_screenshot` grabs every monitor (our own windows excluded,
//! see `lowk-capture`) into an in-memory queue and emits `queue://changed`; the next Ask (M3)
//! takes the queue with [`take_queue`]. Nothing is written to disk here.
use crate::state::{AppState, Shared};
use lowk_capture::CapturedShot;
use lowk_types::*;
use std::sync::Arc;
#[cfg(windows)]
use tauri::Manager;
use tauri::State;

/// Most screenshots kept waiting for an Ask (oldest dropped first).
pub const MAX_QUEUE: usize = 12;

fn emit_queue(st: &AppState) {
    let q = st.shots.lock();
    st.emit(AppEvent::Queue(QueueState {
        shots: q.len() as u32,
        thumbs: q.iter().map(|s| s.thumb.clone()).collect(),
    }));
}

/// Raw handles of this app's windows (what a Windows grab must exclude).
#[cfg(windows)]
fn own_windows(st: &AppState) -> Vec<isize> {
    st.app
        .webview_windows()
        .values()
        .filter_map(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
        .collect()
}
#[cfg(not(windows))]
fn own_windows(st: &AppState) -> Vec<isize> {
    let _ = st;
    vec![]
}

/// Grab all monitors into the queue; returns the queue length. Used by the command and the bind.
pub fn capture_into_queue(st: &Arc<AppState>) -> Result<u32> {
    #[cfg(target_os = "macos")]
    if lowk_platform::perm::status(PermKind::ScreenRecording) != PermState::Granted {
        return Err(AppError::new(ErrorCode::PermScreenRecording));
    }
    let own = own_windows(st);
    let shots = lowk_capture::capture_all(&own)?;
    {
        let mut q = st.shots.lock();
        q.extend(shots);
        let n = q.len();
        if n > MAX_QUEUE {
            q.drain(..n - MAX_QUEUE);
        }
    }
    emit_queue(st);
    Ok(st.shots.lock().len() as u32)
}

pub fn clear_queue(st: &AppState) {
    st.shots.lock().clear();
    emit_queue(st);
}

#[allow(dead_code)] // consumed by the LLM ask in M3
/// The queued screenshots, emptying the queue (called by the LLM ask in M3).
pub fn take_queue(st: &AppState) -> Vec<CapturedShot> {
    let v = std::mem::take(&mut *st.shots.lock());
    emit_queue(st);
    v
}

#[tauri::command(async)]
#[specta::specta]
pub fn capture_screenshot(st: State<'_, Shared>) -> Result<u32> {
    capture_into_queue(&st)
}

#[tauri::command(async)]
#[specta::specta]
pub fn queue_clear(st: State<'_, Shared>) -> Result<()> {
    clear_queue(&st);
    Ok(())
}

#[cfg(feature = "qa")]
/// QA hook: PNG sizes of the queued shots (for the real-app check; no pixels leave the process).
pub fn queue_png_sizes(st: &AppState) -> Vec<(u32, u32, usize)> {
    st.shots.lock().iter().map(|s| (s.width, s.height, s.png.len())).collect()
}
