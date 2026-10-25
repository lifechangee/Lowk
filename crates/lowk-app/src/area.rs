//! Area screenshots (ScreenshotArea bind): a pre-created, hidden, stealth `area` window is moved over the
//! display under the pointer and shown (`AreaCtl`: excluded from sharing and from our own grabs, takes the
//! mouse and keys without activating the app, gives focus back on close). The page only reports the
//! dragged rectangle in window pixels; the geometry (clamp, points -> pixels per display, negative
//! origins) is `lowk_capture::area`, and the grab goes into the same queue as a normal screenshot.
use crate::state::AppState;
use lowk_capture::area::{self as geo, AreaDisplay};
use lowk_input::SystemCursor;
use lowk_types::*;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const AREA_LABEL: &str = lowk_stealth::AREA;
/// A selection nobody finishes closes itself, so a stuck page can never hold the screen.
const OPEN_TIMEOUT: Duration = Duration::from_secs(45);

struct Open {
    display: AreaDisplay,
    gen: u64,
}
static OPEN: Mutex<Option<Open>> = Mutex::new(None);
static GEN: AtomicU64 = AtomicU64::new(1);

/// The displays as the selection window covers them (Tauri reports physical pixels per monitor).
fn displays(app: &AppHandle) -> Vec<AreaDisplay> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let s = m.scale_factor().max(0.1);
            let (p, z) = (m.position(), m.size());
            let (px, py, pw, ph) = (
                f64::from(p.x),
                f64::from(p.y),
                f64::from(z.width),
                f64::from(z.height),
            );
            AreaDisplay {
                points: Rect {
                    x: px / s,
                    y: py / s,
                    w: pw / s,
                    h: ph / s,
                },
                scale: s,
                px_origin: (px, py),
            }
        })
        .collect()
}

fn lang(st: &AppState) -> &'static str {
    match st.settings.get().ui.language {
        UiLanguage::Ru => "ru",
        UiLanguage::En => "en",
        UiLanguage::Auto => {
            if lowk_platform::locale::system()
                .trim()
                .to_lowercase()
                .starts_with("ru")
            {
                "ru"
            } else {
                "en"
            }
        }
    }
}

fn place(w: &tauri::WebviewWindow, d: &AreaDisplay) {
    // macOS: global points, converted by the window's own screen. Windows: physical pixels (a logical size would
    // use the DPI of the monitor the window was on before), position set again after the size (DPI change).
    #[cfg(windows)]
    {
        use tauri::{PhysicalPosition, PhysicalSize};
        let pos = PhysicalPosition::new(d.px_origin.0.round() as i32, d.px_origin.1.round() as i32);
        let _ = w.set_position(pos);
        let _ = w.set_size(PhysicalSize::new(
            (d.points.w * d.scale).round() as u32,
            (d.points.h * d.scale).round() as u32,
        ));
        let _ = w.set_position(pos);
    }
    #[cfg(not(windows))]
    {
        use tauri::{LogicalPosition, LogicalSize};
        let _ = w.set_position(LogicalPosition::new(d.points.x, d.points.y));
        let _ = w.set_size(LogicalSize::new(d.points.w, d.points.h));
    }
    // The system cursor stays a plain arrow: any shape change is composited into screen shares. The page draws its own crosshair.
    let _ = w.set_cursor_icon(tauri::CursorIcon::Default);
}

/// The bind: show the selection over the display under the pointer. A second press while it is open does nothing.
pub fn open(st: &Arc<AppState>) -> Result<()> {
    #[cfg(target_os = "macos")]
    if lowk_platform::perm::status(PermKind::ScreenRecording) != PermState::Granted {
        return Err(AppError::new(ErrorCode::PermScreenRecording));
    }
    let area = st
        .stealth()?
        .area
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::StealthUnavailable).with_detail("area window"))?;
    let mut slot = OPEN.lock();
    if slot.is_some() {
        return Ok(());
    }
    let ds = displays(&st.app);
    let idx = geo::display_at(&ds, SystemCursor.position(), cfg!(windows))
        .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("no display"))?;
    let display = ds[idx];
    let w = st
        .app
        .get_webview_window(AREA_LABEL)
        .ok_or_else(|| AppError::new(ErrorCode::StealthUnavailable).with_detail("area window"))?;
    place(&w, &display);
    let keys = area.open()?;
    let gen = GEN.fetch_add(1, Ordering::SeqCst);
    *slot = Some(Open { display, gen });
    drop(slot);
    let _ = w.emit(
        "area://open",
        serde_json::json!({ "lang": lang(st), "keys": keys, "w": display.points.w, "h": display.points.h }),
    );
    let st2 = st.clone();
    std::thread::spawn(move || {
        std::thread::sleep(OPEN_TIMEOUT);
        if OPEN.lock().as_ref().is_some_and(|o| o.gen == gen) {
            tracing::info!("area selection timed out");
            cancel(&st2);
        }
    });
    Ok(())
}

fn close(st: &AppState) {
    // The selection window ate the key-ups: end any modifier hold (Ctrl click-hold, Alt move-hold) instead of leaving it stuck.
    st.input.lock().reset_holds();
    if let Some(a) = st.stealth.get().and_then(|s| s.area.as_ref()) {
        a.close();
    }
}

/// Esc, right-click, a click without a drag, the timeout: close without a grab.
pub fn cancel(st: &AppState) {
    if OPEN.lock().take().is_some() {
        close(st);
    }
}

/// Release / Enter: `local` is the dragged rectangle in window pixels. Closes the window first (focus goes back
/// to the app that had it), then grabs exactly that rectangle into the screenshot queue.
pub fn confirm(st: &Arc<AppState>, local: Rect) -> Result<()> {
    let Some(open) = OPEN.lock().take() else {
        return Ok(());
    };
    close(st);
    let pick = geo::resolve(&open.display, local)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument).with_detail("area too small"))?;
    let rect = if cfg!(windows) { pick.pixels } else { pick.points };
    let r = crate::commands::capture::capture_area_into_queue(st, rect);
    if let Err(e) = &r {
        tracing::warn!(%e, "area screenshot failed");
        if e.code == ErrorCode::PermScreenRecording {
            st.emitter.broadcast(
                "binds://failed",
                serde_json::json!({ "action": ActionId::ScreenshotArea, "perm": "screen_recording" }),
            );
        }
    }
    r.map(|_| ())
}
