//! Overlay / pill visibility policy (`settings.overlay.visibility`).
use crate::state::AppState;
use lowk_types::*;
use std::sync::atomic::Ordering;
use tauri::Manager;

/// What the overlay and the pill should do for a phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Keep,
    Show,
    Hide,
}

/// Pure policy: (overlay, pill, forget the manual hide). A call that is `Stopping` is already over for
/// the user: the overlay (and its «Завершить» / microphone controls) goes away at once, exactly as when
/// the call is Idle, while the transcript, the STT workers and the call file are finalized in the
/// background. Waiting for `Idle` kept the window on screen for 5-10 s (worker flush, correction grace).
pub fn plan(vis: OverlayVisibility, phase: SessionPhase, user_hidden: bool) -> (Act, Act, bool) {
    let active = !matches!(phase, SessionPhase::Idle | SessionPhase::Stopping);
    match (vis, active) {
        (OverlayVisibility::Manual, _) => (Act::Keep, Act::Keep, false),
        (_, true) => (if user_hidden { Act::Keep } else { Act::Show }, Act::Hide, false),
        (OverlayVisibility::Always, false) => (Act::Show, Act::Hide, false),
        (OverlayVisibility::RecordingOnly, false) => (Act::Hide, Act::Hide, true),
        (OverlayVisibility::Pill, false) => (Act::Hide, Act::Show, true),
    }
}

/// Bring the overlay and pill in line with the session phase and the setting.
/// Runs on the driver thread (never on an event-producing thread).
pub fn apply(st: &AppState, phase: SessionPhase) {
    let t = std::time::Instant::now();
    let Ok(stealth) = st.stealth() else { return };
    let ov = st.settings.get().overlay;
    let (overlay, pill, forget_hidden) = plan(
        ov.visibility,
        phase,
        st.overlay_user_hidden.load(Ordering::SeqCst),
    );
    if forget_hidden {
        st.overlay_user_hidden.store(false, Ordering::SeqCst);
    }
    match overlay {
        Act::Show => {
            if let Some(p) = &stealth.pill {
                p.hide();
            }
            if let Err(e) = stealth.controller.show() {
                tracing::error!(%e, "overlay not shown (failed closed)");
            }
        }
        Act::Hide => {
            stealth.controller.hide();
            tracing::info!(?phase, ms = t.elapsed().as_millis() as u64, "overlay hidden");
        }
        Act::Keep => {}
    }
    match pill {
        Act::Hide if overlay != Act::Show => {
            if let Some(p) = &stealth.pill {
                p.hide();
            }
        }
        Act::Show => {
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
        _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use OverlayVisibility as V;
    use SessionPhase as P;

    #[test]
    fn the_overlay_hides_when_the_call_starts_stopping_not_when_finalization_ends() {
        for vis in [V::RecordingOnly, V::Pill] {
            let (ov, _, forget) = plan(vis, P::Stopping, false);
            assert_eq!(ov, Act::Hide, "{vis:?}: still on screen while Stopping");
            assert!(forget);
            assert_eq!(plan(vis, P::Idle, false).0, Act::Hide);
        }
    }

    #[test]
    fn stopping_and_idle_agree_for_every_setting() {
        for vis in [V::Always, V::RecordingOnly, V::Pill, V::Manual] {
            for hidden in [false, true] {
                assert_eq!(
                    plan(vis, P::Stopping, hidden),
                    plan(vis, P::Idle, hidden),
                    "{vis:?}"
                );
            }
        }
    }

    #[test]
    fn a_running_call_shows_the_overlay_unless_hidden_by_hand() {
        for p in [P::Starting, P::Recording, P::Paused] {
            assert_eq!(plan(V::RecordingOnly, p, false), (Act::Show, Act::Hide, false));
            assert_eq!(plan(V::RecordingOnly, p, true).0, Act::Keep);
        }
        assert_eq!(plan(V::Manual, P::Recording, false).0, Act::Keep);
        assert_eq!(plan(V::Pill, P::Idle, false), (Act::Hide, Act::Show, true));
    }
}
