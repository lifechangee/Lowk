//! Stealth-probe control hook (debug builds only). With `LOWK_PROBE_CTL=<file>`
//! the app polls that file for scenario commands, and `LOWK_PROBE=1` makes the
//! windows load solid sentinel pages (overlay magenta, main green), so
//! `tools/stealth-probe` can regression-test the REAL windows.
//!
//!   overlay-show | overlay-hide | main-show | main-hide | main-destroy | panic
//!   overlay-move X Y | mode passive|interactive | magnet X0 Y0 X1 Y1
//!   exclusion on|off | hide-main on|off | policy-flip | state | quit
use crate::state::Shared;
use lowk_stealth::{WindowPolicy, MAIN, OVERLAY};
use lowk_types::*;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

pub fn enabled() -> bool {
    std::env::var_os("LOWK_PROBE").is_some()
}

pub fn spawn(st: Shared) {
    let Some(ctl) = std::env::var_os("LOWK_PROBE_CTL").map(PathBuf::from) else {
        return;
    };
    let log = ctl.with_extension("log");
    std::thread::Builder::new()
        .name("lowk-probe".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(100));
            let Ok(text) = std::fs::read_to_string(&ctl) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            let _ = std::fs::write(&ctl, "");
            for line in text.lines() {
                let msg = run(&st, line.trim());
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
                    let _ = writeln!(f, "{line} -> {msg}");
                }
            }
        })
        .expect("spawn probe");
}

fn run(st: &Shared, line: &str) -> String {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let Ok(stealth) = st.stealth() else {
        return "no stealth".into();
    };
    let f = |i: usize| parts.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    match parts.as_slice() {
        ["overlay-show"] => format!("{:?}", stealth.controller.show().map_err(|e| e.to_string())),
        ["overlay-hide"] => {
            stealth.controller.hide();
            "ok".into()
        }
        ["main-show"] => {
            crate::windows::open_main(&st.app);
            "ok".into()
        }
        ["main-hide"] => {
            if let Some(w) = st.app.get_webview_window(MAIN) {
                let _ = w.hide();
            }
            "ok".into()
        }
        ["main-destroy"] => {
            if let Some(w) = st.app.get_webview_window(MAIN) {
                let _ = w.destroy();
            }
            "ok".into()
        }
        ["panic"] => {
            stealth.controller.panic_hide();
            "ok".into()
        }
        ["overlay-move", ..] => {
            stealth.overlay.set_origin(f(1), f(2));
            "ok".into()
        }
        ["mode", m] => {
            let mode = if *m == "interactive" {
                OverlayMode::Interactive
            } else {
                OverlayMode::Passive
            };
            format!(
                "{:?}",
                stealth.controller.set_mode(mode).map_err(|e| e.to_string())
            )
        }
        ["magnet", ..] => {
            // Scripted cursor: press at (x0,y0), drag to (x1,y1), release.
            struct Path(parking_lot::Mutex<(f64, f64)>);
            impl CursorSource for Path {
                fn position(&self) -> (f64, f64) {
                    *self.0.lock()
                }
            }
            let cur = Arc::new(Path(parking_lot::Mutex::new((f(1), f(2)))));
            stealth.overlay.move_hold_with(Phase::Down, cur.clone());
            std::thread::sleep(Duration::from_millis(150));
            *cur.0.lock() = (f(3), f(4));
            std::thread::sleep(Duration::from_millis(300));
            stealth.overlay.move_hold_with(Phase::Up, cur);
            "ok".into()
        }
        ["exclusion", v] => {
            let mut p = WindowPolicy::OVERLAY;
            p.capture_excluded = *v == "on";
            format!(
                "{:?}",
                stealth.guard.set_policy(OVERLAY, p).map_err(|e| e.to_string())
            )
        }
        ["hide-main", v] => format!(
            "{:?}",
            stealth.set_hide_main(*v == "on").map_err(|e| e.to_string())
        ),
        ["policy-flip"] => {
            #[cfg(target_os = "macos")]
            {
                let _ = st.app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                std::thread::sleep(Duration::from_millis(300));
                let _ = st.app.set_activation_policy(tauri::ActivationPolicy::Regular);
                stealth.guard.on_policy_change();
            }
            "ok".into()
        }
        ["state"] => format!(
            "status={:?} mode={:?} selftest={:?}",
            stealth.controller.status(),
            stealth.controller.mode(),
            stealth.controller.selftest().map_err(|e| e.to_string())
        ),
        ["quit"] => {
            st.app.exit(0);
            "bye".into()
        }
        _ => "unknown".into(),
    }
}
