//! Overlay / pill visibility policy (`settings.overlay.visibility`).
use crate::state::AppState;
use lowk_types::*;
use std::sync::atomic::Ordering;
use tauri::Manager;

/// Bring the overlay and pill in line with the session phase and the setting.
/// Runs on the driver thread (never on an event-producing thread).
pub fn apply(st: &AppState, phase: SessionPhase) {
    let Ok(stealth) = st.stealth() else { return };
    let ov = st.settings.get().overlay;
    let active = phase != SessionPhase::Idle;
    let hide_pill = || {
        if let Some(p) = &stealth.pill {
            p.hide();
        }
    };
    match (ov.visibility, active) {
        (OverlayVisibility::Manual, _) => {}
        (_, true) => {
            hide_pill();
            if !st.overlay_user_hidden.load(Ordering::SeqCst) {
                if let Err(e) = stealth.controller.show() {
                    tracing::error!(%e, "overlay not shown (failed closed)");
                }
            }
        }
        (OverlayVisibility::Always, false) => {
            hide_pill();
            if let Err(e) = stealth.controller.show() {
                tracing::error!(%e, "overlay not shown (failed closed)");
            }
        }
        (OverlayVisibility::RecordingOnly, false) => {
            st.overlay_user_hidden.store(false, Ordering::SeqCst);
            stealth.controller.hide();
            hide_pill();
        }
        (OverlayVisibility::Pill, false) => {
            st.overlay_user_hidden.store(false, Ordering::SeqCst);
            stealth.controller.hide();
            if let Some(p) = &stealth.pill {
                if let Some(w) = st.app.get_webview_window(lowk_stealth::PILL) {
                    let (pw, ph) = crate::windows::pill_size(ov.pill_edge);
                    let _ = w.set_size(tauri::LogicalSize::new(pw, ph));
                }
                let saved = ov.pill_positions.clone();
                let lookup = move |d: &Rect| {
                    saved
                        .get(&lowk_stealth::magnet::display_key(d))
                        .map(|p| (p.x, p.y))
                };
                if let Err(e) = p.show_with(ov.pill_edge, lookup) {
                    tracing::error!(%e, "pill not shown (failed closed)");
                }
            }
        }
    }
}

/// `ToggleOverlay` bind / `overlay_show` / `overlay_hide`.
pub fn toggle(st: &AppState) -> Result<()> {
    let stealth = st.stealth()?;
    if stealth.guard.is_visible(lowk_stealth::OVERLAY) {
        st.overlay_user_hidden.store(true, Ordering::SeqCst);
        stealth.controller.hide();
    } else {
        st.overlay_user_hidden.store(false, Ordering::SeqCst);
        if let Some(p) = &stealth.pill {
            p.hide();
        }
        stealth.controller.show()?;
    }
    Ok(())
}

/// F8: after a recording ends, show/focus the main window and tell it to go to
/// the library home (`session://ended`; the UI routes to `#/`).
pub fn session_ended(st: &AppState) {
    crate::windows::open_main(&st.app);
    st.emitter.broadcast(
        lowk_types::events::SESSION_ENDED,
        serde_json::json!({ "route": "home" }),
    );
}
