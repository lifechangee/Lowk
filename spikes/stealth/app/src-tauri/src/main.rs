// THROWAWAY SPIKE: stealth overlay vs capture. Not product code.
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as platform;
#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as platform;
#[cfg(not(any(target_os = "macos", windows)))]
mod other;
#[cfg(not(any(target_os = "macos", windows)))]
use other as platform;

pub fn log(msg: impl AsRef<str>) {
    let line = format!("[{:?}] {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(), msg.as_ref());
    println!("{line}");
    let path = std::env::var("LOWK_SPIKE_LOG").unwrap_or_else(|_| "spike.log".into());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

static PANIC_HIDDEN: AtomicBool = AtomicBool::new(false);

/// Re-apply exclusion on the main thread and read it back. Rust owns this.
fn reapply(app: &AppHandle, reason: &str) {
    let app2 = app.clone();
    let reason = reason.to_string();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = app2.get_webview_window("overlay") {
            let r = platform::apply_exclusion(&w);
            log(format!("CHECK reason={reason} -> {r}"));
        }
    });
}

fn overlay_show(app: &AppHandle) {
    // AppKit calls must be on the main thread (calling after_show from the watchdog thread hung it)
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = a.get_webview_window("overlay") {
            let _ = w.show();
            platform::after_show(&w);
            PANIC_HIDDEN.store(false, Ordering::SeqCst);
            let r = platform::apply_exclusion(&w);
            log(format!("CHECK reason=show -> {r}"));
        }
    });
}
fn overlay_hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
        PANIC_HIDDEN.store(true, Ordering::SeqCst);
        log("overlay hidden (panic/hide)");
    }
}

fn run_cmd(app: &AppHandle, cmd: &str) {
    log(format!("CMD {cmd}"));
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    match parts.as_slice() {
        ["main-hide"] => { if let Some(w) = app.get_webview_window("main") { let _ = w.hide(); } reapply(app, "main-hide"); }
        ["main-show"] => { if let Some(w) = app.get_webview_window("main") { let _ = w.show(); } reapply(app, "main-show"); }
        ["main-focus"] => { if let Some(w) = app.get_webview_window("main") { let _ = w.show(); let _ = w.set_focus(); } reapply(app, "main-focus"); }
        ["overlay-hide"] => overlay_hide(app),
        ["overlay-show"] => overlay_show(app),
        ["overlay-move", x, y] => {
            if let (Ok(x), Ok(y), Some(w)) = (x.parse::<f64>(), y.parse::<f64>(), app.get_webview_window("overlay")) {
                let _ = w.set_position(tauri::LogicalPosition::new(x, y));
            }
        }
        ["overlay-toggle-exclusion", on] => {
            // negative control: lets the probe prove it CAN see the marker
            let on = *on == "on";
            if let Some(w) = app.get_webview_window("overlay") { platform::set_excluded_default(on); let _ = w; }
            reapply(app, "toggle-exclusion");
        }
        ["quit"] => app.exit(0),
        _ => log(format!("unknown cmd {cmd}")),
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, sc, ev| {
                    if ev.state() != ShortcutState::Pressed { return; }
                    let s = sc.to_string().to_lowercase();
                    log(format!("HOTKEY {s}"));
                    if s.contains("keyh") {
                        if PANIC_HIDDEN.load(Ordering::SeqCst) { overlay_show(app) } else { overlay_hide(app) }
                    } else if s.contains("keym") {
                        let vis = app.get_webview_window("main").and_then(|w| w.is_visible().ok()).unwrap_or(false);
                        run_cmd(app, if vis { "main-hide" } else { "main-show" });
                    }
                })
                .build(),
        )
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let h = app.handle().clone();
            let overlay: WebviewWindow = app.get_webview_window("overlay").unwrap();
            platform::setup_overlay(&overlay);
            reapply(&h, "setup");
            let _ = overlay.show();
            platform::after_show(&overlay);
            reapply(&h, "initial-show");

            // events -> re-apply
            let h2 = h.clone();
            overlay.on_window_event(move |ev| {
                let r = match ev {
                    WindowEvent::Moved(_) => "moved",
                    WindowEvent::Resized(_) => "resized",
                    WindowEvent::Focused(_) => "focused",
                    WindowEvent::ScaleFactorChanged { .. } => "scale",
                    _ => return,
                };
                reapply(&h2, r);
            });
            platform::observe_space_changes(h.clone(), |app| { let a = app.clone(); let _ = app.run_on_main_thread(move || { if let Some(w) = a.get_webview_window("overlay") { platform::after_show(&w); } }); reapply(app, "space-change") });

            for k in ["CommandOrControl+Shift+H", "CommandOrControl+Shift+M"] {
                if let Err(e) = app.global_shortcut().register(k) { log(format!("hotkey {k} register failed: {e}")); }
            }

            // 1s watchdog + control-file poll
            let ctl = std::env::var("LOWK_SPIKE_CTL").unwrap_or_else(|_| "spike.ctl".into());
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(250));
                if let Ok(c) = std::fs::read_to_string(&ctl) {
                    let _ = std::fs::remove_file(&ctl);
                    for l in c.lines() { run_cmd(&h, l.trim()); }
                }
                static TICK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                if TICK.fetch_add(1, Ordering::SeqCst) % 4 == 0 { reapply(&h, "watchdog"); }
            });
            log("setup complete");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run");
}
