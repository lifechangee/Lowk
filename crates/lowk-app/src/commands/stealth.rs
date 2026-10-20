//! stealth status, overlay control. Every window flag is set in Rust; the
//! frontend has no `core:window` permission at all.
use crate::state::{AppState, Shared};
use lowk_types::*;
use std::sync::Arc;
use tauri::{Emitter, State};

#[tauri::command(async)]
#[specta::specta]
pub fn stealth_status(st: State<'_, Shared>) -> StealthStatus {
    st.stealth()
        .map(|s| s.controller.status())
        .unwrap_or_else(|e| StealthStatus::Unavailable(e.to_string()))
}

/// Paints the overlay sentinel, grabs the screen like a normal capturer and
/// checks the sentinel is absent (needs Screen Recording on macOS).
#[tauri::command(async)]
#[specta::specta]
pub async fn stealth_selftest(st: State<'_, Shared>) -> Result<SelfTestReport> {
    let stealth = st.stealth()?.clone();
    let app = st.app.clone();
    let _ = app.emit_to(
        tauri::EventTarget::webview_window("overlay"),
        "overlay://sentinel",
        true,
    );
    tokio_sleep(450).await;
    let r = tauri::async_runtime::spawn_blocking(move || stealth.controller.selftest())
        .await
        .map_err(|e| AppError::new(ErrorCode::Unknown).with_detail(e.to_string()))?;
    let _ = app.emit_to(
        tauri::EventTarget::webview_window("overlay"),
        "overlay://sentinel",
        false,
    );
    r
}

async fn tokio_sleep(ms: u64) {
    let _ = tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms))
    })
    .await;
}

#[tauri::command(async)]
#[specta::specta]
pub fn overlay_set_mode(st: State<'_, Shared>, mode: OverlayMode) -> Result<()> {
    st.stealth()?.controller.set_mode(mode)
}

#[tauri::command(async)]
#[specta::specta]
pub fn overlay_show(st: State<'_, Shared>) -> Result<()> {
    st.overlay_user_hidden
        .store(false, std::sync::atomic::Ordering::SeqCst);
    st.stealth()?.controller.show()
}

#[tauri::command(async)]
#[specta::specta]
pub fn overlay_hide(st: State<'_, Shared>) -> Result<()> {
    st.overlay_user_hidden
        .store(true, std::sync::atomic::Ordering::SeqCst);
    st.stealth()?.controller.hide();
    Ok(())
}

/// Esc in the interactive overlay: back to click-through.
#[tauri::command(async)]
#[specta::specta]
pub fn overlay_escape(st: State<'_, Shared>) -> Result<()> {
    st.stealth()?.overlay.escape();
    Ok(())
}

/// Header drag while interactive (plain pointer drag: the overlay keeps
/// receiving mouse events, unlike the hold-to-move magnet which is click-through).
#[tauri::command(async)]
#[specta::specta]
pub fn overlay_drag_start(st: State<'_, Shared>) -> Result<()> {
    let f = st.stealth()?.overlay.frame();
    *st.drag_origin.lock() = Some((f.x, f.y));
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn overlay_drag_to(st: State<'_, Shared>, dx: f64, dy: f64) -> Result<()> {
    if let Some((x, y)) = *st.drag_origin.lock() {
        st.stealth()?.overlay.set_origin(x + dx, y + dy);
    }
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn overlay_drag_end(st: State<'_, Shared>) -> Result<()> {
    if st.drag_origin.lock().take().is_some() {
        remember_position(&st);
    }
    Ok(())
}

/// Interactive-mode input row: ask the AI with the typed text.
#[tauri::command(async)]
#[specta::specta]
pub fn overlay_input_submit(st: State<'_, Shared>, text: String) -> Result<QaId> {
    let template_id = st.template.lock().clone();
    st.session.ask(AskRequest {
        template_id,
        typed_text: Some(text),
    })
}

/// Persist the overlay origin for the display it is on (`primary` in M0).
pub fn remember_position(st: &Arc<AppState>) {
    let Ok(stealth) = st.stealth() else { return };
    let f = stealth.overlay.frame();
    let display = st.settings.get().overlay.display;
    let _ = st.settings.patch(SettingsPatch(serde_json::json!({
        "overlay": { "positions": { display: { "x": f.x, "y": f.y } } }
    })));
}

impl AppState {
    /// Move the overlay to its remembered origin, else top-centre of the primary display.
    pub fn place_overlay(&self) {
        let Ok(stealth) = self.stealth() else { return };
        let ov = self.settings.get().overlay;
        let frames = stealth.overlay.display_frames();
        let Some(primary) = frames.first().copied() else {
            return;
        };
        let (x, y) = match ov.positions.get(&ov.display) {
            Some(p)
                if frames
                    .iter()
                    .any(|d| p.x >= d.x && p.x < d.x + d.w && p.y >= d.y && p.y < d.y + d.h) =>
            {
                (p.x, p.y)
            }
            _ => (
                primary.x + (primary.w - crate::windows::OVERLAY_SIZE.0) / 2.0,
                primary.y + 48.0,
            ),
        };
        stealth.overlay.set_origin(x, y);
    }
}

/// Current adaptive tone (a window that loads late missed the events).
#[tauri::command(async)]
#[specta::specta]
pub fn overlay_tone(st: State<'_, Shared>) -> Result<Tone> {
    Ok(st.stealth()?.current_tone())
}
