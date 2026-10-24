//! G7: screenshot queue. `capture_screenshot` grabs the active monitor (or all / the primary, per
//! `capture.monitors`; our own windows excluded, see `lowk-capture`) into an in-memory queue and emits
//! `queue://changed`; the next Ask sends the queue to the model (`ShotSource`). Nothing is written to disk here.
use crate::emitter::UiEmitter;
use crate::state::{AppState, Shared};
use lowk_capture::CapturedShot;
use lowk_session::ShotSource;
use lowk_types::CursorSource;
use lowk_types::*;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
#[cfg(windows)]
use tauri::Manager;
use tauri::State;

/// Most screenshots kept waiting for an Ask (oldest dropped first).
pub const MAX_QUEUE: usize = 12;

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

/// The screenshots waiting for the next Ask (in memory only). The session reads it through
/// [`ShotSource`]: an Ask takes a copy, removes exactly what it sent once it succeeded, and a
/// stopped session empties it.
pub struct ShotQueue {
    q: Mutex<Vec<CapturedShot>>,
    next_id: AtomicU64,
    emitter: Arc<UiEmitter>,
    /// Grabs the active display(s) for a vision template with an empty queue (set after boot).
    grab: OnceLock<Box<dyn Fn() -> Result<Vec<CapturedShot>> + Send + Sync>>,
}

impl ShotQueue {
    pub fn new(emitter: Arc<UiEmitter>) -> Self {
        Self {
            q: Mutex::new(vec![]),
            next_id: AtomicU64::new(1),
            emitter,
            grab: OnceLock::new(),
        }
    }

    pub fn set_grab(&self, f: Box<dyn Fn() -> Result<Vec<CapturedShot>> + Send + Sync>) {
        let _ = self.grab.set(f);
    }

    pub fn lock(&self) -> parking_lot::MutexGuard<'_, Vec<CapturedShot>> {
        self.q.lock()
    }

    fn emit(&self) {
        let ev = {
            let q = self.q.lock();
            AppEvent::Queue(QueueState {
                shots: q.len() as u32,
                thumbs: q.iter().map(|s| s.thumb.clone()).collect(),
            })
        };
        self.emitter.emit(ev);
    }

    /// Adds shots (ids assigned here), dropping the oldest beyond [`MAX_QUEUE`].
    pub fn push(&self, shots: Vec<CapturedShot>) {
        {
            let mut q = self.q.lock();
            for mut s in shots {
                s.id = self.next_id.fetch_add(1, Ordering::SeqCst);
                q.push(s);
            }
            let n = q.len();
            if n > MAX_QUEUE {
                q.drain(..n - MAX_QUEUE);
            }
        }
        self.emit();
    }
}

fn queued(s: &CapturedShot) -> QueuedImage {
    QueuedImage {
        id: s.id,
        index: s.index,
        primary: s.primary,
        width: s.width,
        height: s.height,
        png: s.png.clone(),
    }
}

impl ShotSource for ShotQueue {
    fn peek(&self) -> Vec<QueuedImage> {
        self.q.lock().iter().map(queued).collect()
    }
    fn capture_if_empty(&self) -> Vec<QueuedImage> {
        let Some(grab) = self.grab.get() else {
            return vec![];
        };
        match grab() {
            // taken for this Ask only: not shown in the queue, removed again by `consume`
            Ok(mut shots) => {
                for s in &mut shots {
                    s.id = self.next_id.fetch_add(1, Ordering::SeqCst);
                }
                shots.iter().map(queued).collect()
            }
            Err(e) => {
                tracing::warn!(code = ?e.code, "vision template: no screenshot taken");
                vec![]
            }
        }
    }
    fn consume(&self, ids: &[u64]) {
        let changed = {
            let mut q = self.q.lock();
            let n = q.len();
            q.retain(|s| !ids.contains(&s.id));
            q.len() != n
        };
        if changed {
            self.emit();
        }
    }
    fn clear(&self) {
        let changed = {
            let mut q = self.q.lock();
            let had = !q.is_empty();
            q.clear();
            had
        };
        if changed {
            self.emit();
        }
    }
}

fn grab_now(st: &AppState) -> Result<Vec<CapturedShot>> {
    #[cfg(target_os = "macos")]
    if lowk_platform::perm::status(PermKind::ScreenRecording) != PermState::Granted {
        return Err(AppError::new(ErrorCode::PermScreenRecording));
    }
    let own = own_windows(st);
    let mode = st.settings.get().capture.monitors;
    let mouse = Some(lowk_input::SystemCursor.position());
    lowk_capture::capture_all(&own, mode, mouse)
}

/// Grab the displays picked by `capture.monitors` (default: the active one) into the queue; returns
/// the queue length. Used by the command and the bind.
pub fn capture_into_queue(st: &Arc<AppState>) -> Result<u32> {
    let shots = grab_now(st)?;
    st.shots.push(shots);
    Ok(st.shots.lock().len() as u32)
}

/// The grab used by vision templates with an empty queue.
pub fn grab_for_ask(st: &Arc<AppState>) -> Result<Vec<CapturedShot>> {
    grab_now(st)
}

/// A rectangle picked in the area selection (global top-left points on macOS, physical pixels on Windows)
/// into the queue.
pub fn capture_area_into_queue(st: &Arc<AppState>, rect: Rect) -> Result<u32> {
    #[cfg(target_os = "macos")]
    if lowk_platform::perm::status(PermKind::ScreenRecording) != PermState::Granted {
        return Err(AppError::new(ErrorCode::PermScreenRecording));
    }
    let shot = lowk_capture::capture_area(&own_windows(st), rect)?;
    st.shots.push(vec![shot]);
    Ok(st.shots.lock().len() as u32)
}

pub fn clear_queue(st: &AppState) {
    ShotSource::clear(&*st.shots);
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

/// Area selection result: the dragged rectangle in window pixels (the page cannot name a display or a screen
/// position; Rust knows which display the window covers).
#[tauri::command(async)]
#[specta::specta]
pub fn area_confirm(st: State<'_, Shared>, rect: Rect) -> Result<()> {
    crate::area::confirm(&st, rect)
}

/// Esc / right-click / click without a drag.
#[tauri::command(async)]
#[specta::specta]
pub fn area_cancel(st: State<'_, Shared>) -> Result<()> {
    crate::area::cancel(&st);
    Ok(())
}

#[cfg(feature = "qa")]
/// QA hook: PNG sizes of the queued shots (for the real-app check; no pixels leave the process).
pub fn queue_png_sizes(st: &AppState) -> Vec<(u32, u32, usize)> {
    st.shots
        .lock()
        .iter()
        .map(|s| (s.width, s.height, s.png.len()))
        .collect()
}

#[cfg(feature = "qa")]
/// QA hook: the i-th queued PNG, base64 (the real-app check decodes it and proves our windows are absent).
pub fn queue_png_b64(st: &AppState, i: usize) -> Option<String> {
    st.shots.lock().get(i).map(|s| lowk_capture::base64(&s.png))
}
