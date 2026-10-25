//! Window creation and lifecycle. The overlay (and the pill) are created
//! hidden at startup, protected through `lowk-stealth` before their first show,
//! and never destroyed. The main window is created on demand and destroyed on
//! close (frees the WebView); the Dock icon exists only while it is open.
use crate::state::Shared;
use lowk_stealth::{RawWindow, AREA, MAIN, OVERLAY, PILL};
use lowk_types::*;
use raw_window_handle::HasWindowHandle;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent};

pub const OVERLAY_SIZE: (f64, f64) = (600.0, 384.0);

/// Probe mode (`LOWK_PROBE=1`, debug builds): solid sentinel pages for tools/stealth-probe.
fn page(name: &str) -> String {
    #[cfg(debug_assertions)]
    if crate::probe::enabled() {
        return format!("{name}#probe");
    }
    name.to_string()
}

pub fn raw_window(w: &WebviewWindow) -> Result<RawWindow> {
    let h = w
        .window_handle()
        .map_err(|e| AppError::new(ErrorCode::StealthUnavailable).with_detail(e.to_string()))?;
    Ok(RawWindow::new(h.as_raw()))
}

fn err(e: tauri::Error) -> AppError {
    AppError::new(ErrorCode::Unknown).with_detail(e.to_string())
}

/// Hidden, transparent, content-protected from birth (flags are re-applied and
/// read back by the stealth guard before the first show).
pub fn build_overlay(app: &AppHandle) -> Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App(page("overlay.html").into()))
        .title("Lowk Overlay")
        .inner_size(OVERLAY_SIZE.0, OVERLAY_SIZE.1)
        .position(120.0, 80.0)
        .visible(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(false)
        .visible_on_all_workspaces(true)
        .content_protected(true)
        // Holding Ctrl (click-hold bind) makes every wheel event a zoom gesture in WebView2; the page scrolls itself.
        .zoom_hotkeys_enabled(false)
        .build()
        .map_err(err)
}

pub fn build_pill(app: &AppHandle) -> Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, PILL, WebviewUrl::App("overlay.html#pill".into()))
        .title("Lowk")
        .inner_size(140.0, 22.0)
        .position(0.0, 0.0)
        .visible(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(false)
        .visible_on_all_workspaces(true)
        .content_protected(true)
        .build()
        .map_err(err)
}

/// The area-selection window: hidden, transparent, stealth from birth; `area::open` moves it over a display
/// and shows it, so the selection appears without a page load.
pub fn build_area(app: &AppHandle) -> Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, AREA, WebviewUrl::App("area.html".into()))
        .title("Lowk Area")
        .inner_size(800.0, 600.0)
        .position(0.0, 0.0)
        .visible(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(false)
        .accept_first_mouse(true)
        .visible_on_all_workspaces(true)
        .content_protected(true)
        .build()
        .map_err(err)
}

/// Pill window size for an edge: a strip along the edge.
pub fn pill_size(edge: PillEdge) -> (f64, f64) {
    match edge {
        PillEdge::Left | PillEdge::Right => (22.0, 88.0),
        PillEdge::Top | PillEdge::Bottom => (140.0, 22.0),
    }
}

/// Show (create when needed) and focus the main window. Safe from any thread.
pub fn open_main(app: &AppHandle) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(e) = open_main_inner(&a) {
            tracing::error!(%e, "cannot open the main window");
        }
    });
}

fn open_main_inner(app: &AppHandle) -> Result<()> {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    // The Dock icon appears with the main window.
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    let w = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App(page("main.html").into()))
        .title("Lowk")
        .inner_size(1100.0, 720.0)
        .min_inner_size(800.0, 520.0)
        .visible(false)
        .build()
        .map_err(err)?;
    if let Some(st) = app.try_state::<Shared>() {
        if let Ok(stealth) = st.stealth() {
            let hide = st.settings.get().stealth.hide_main;
            if let Err(e) = raw_window(&w).and_then(|r| stealth.register_main(r, hide)) {
                tracing::error!(%e, "main window not registered with the stealth guard");
            }
            stealth.guard.on_policy_change();
        }
    }
    let _ = w.show();
    let _ = w.set_focus();
    Ok(())
}

/// Global window events (all windows).
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let app = window.app_handle();
    let label = window.label();
    match event {
        WindowEvent::CloseRequested { api, .. } if label == OVERLAY || label == PILL => {
            api.prevent_close();
            if let Some(st) = app.try_state::<Shared>() {
                if let Ok(s) = st.stealth() {
                    s.controller.hide();
                }
            }
        }
        WindowEvent::CloseRequested { api, .. } if label == AREA => {
            api.prevent_close();
            if let Some(st) = app.try_state::<Shared>() {
                crate::area::cancel(&st);
            }
        }
        WindowEvent::CloseRequested { .. } if label == MAIN => {
            // Forget the native handle before the window dies.
            if let Some(st) = app.try_state::<Shared>() {
                if let Ok(s) = st.stealth() {
                    s.unregister_main();
                }
            }
        }
        WindowEvent::Destroyed if label == MAIN => {
            if let Some(st) = app.try_state::<Shared>() {
                if let Ok(s) = st.stealth() {
                    s.unregister_main();
                }
            }
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            if let Some(st) = app.try_state::<Shared>() {
                if let Ok(s) = st.stealth() {
                    s.guard.on_policy_change();
                }
            }
        }
        _ => {}
    }
    forward_to_stealth(app, label, event);
}

fn forward_to_stealth(app: &AppHandle, label: &str, event: &WindowEvent) {
    use lowk_stealth::WindowEventKind as K;
    let kind = match event {
        WindowEvent::Focused(f) => K::Focused(*f),
        WindowEvent::Moved(_) => K::Moved,
        WindowEvent::Resized(_) => K::Resized,
        WindowEvent::ScaleFactorChanged { .. } => K::ScaleFactorChanged,
        WindowEvent::ThemeChanged(_) => K::ThemeChanged,
        _ => return,
    };
    let Some(st) = app.try_state::<Shared>() else {
        return;
    };
    let Ok(s) = st.stealth() else { return };
    // The main window changes constantly (drag, resize); only focus matters to the guard.
    if label == MAIN && !matches!(kind, K::Focused(_)) {
        return;
    }
    if label == OVERLAY || label == MAIN || label == PILL {
        s.on_window_event(label, kind);
    }
}
