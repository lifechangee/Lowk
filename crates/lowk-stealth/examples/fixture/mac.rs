//! Stealth fixture for `tools/stealth-probe` (macOS): two raw AppKit windows (no
//! webview), a magenta OVERLAY and a green MAIN, driven by the real
//! `lowk-stealth` code through `create_stealth`.
//!
//!   cargo run -p lowk-stealth --example fixture -- <control-file> [log-file]
//!
//! Commands (one per line, written to the control file, polled every 100 ms):
//!   overlay-show | overlay-hide | main-show | main-hide | panic
//!   overlay-move X Y        overlay origin, global top-left points
//!   mode passive|interactive|escape
//!   magnet X0 Y0 X1 Y1      scripted cursor: press at (X0,Y0), drag to (X1,Y1), release
//!   exclusion on|off        overlay policy; `off` is the probe's NEGATIVE CONTROL
//!   hide-main on|off        "hide main from capture" setting
//!   state | selftest        print diagnostics
//!   quit

use lowk_stealth::*;
use lowk_types::fakes::{CollectEventSink, FixedCursor};
use lowk_types::*;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::*;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSThread};
use raw_window_handle::{AppKitWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use std::io::Write;
use std::ptr::NonNull;
use std::sync::Arc;
use std::time::Duration;

extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, ctx: *mut c_void, work: extern "C" fn(*mut c_void));
}

struct Main;
impl MainThread for Main {
    fn run(&self, f: Box<dyn FnOnce() + Send>) {
        extern "C" fn tramp(ctx: *mut c_void) {
            // SAFETY: ctx is the Box leaked below.
            let f = unsafe { Box::from_raw(ctx as *mut Box<dyn FnOnce() + Send>) };
            f();
        }
        let ctx = Box::into_raw(Box::new(f)) as *mut c_void;
        // SAFETY: main queue is a process-lifetime global.
        unsafe { dispatch_async_f(&raw const _dispatch_main_q, ctx, tramp) };
    }
    fn is_main(&self) -> bool {
        NSThread::isMainThread_class()
    }
}

fn log(msg: impl AsRef<str>) {
    let line = format!(
        "[{}] {}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        msg.as_ref()
    );
    println!("{line}");
    if let Ok(p) = std::env::var("LOWK_FIXTURE_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn make_window(
    mtm: MainThreadMarker,
    rect: NSRect,
    color: (f64, f64, f64),
    title: &str,
    mask: NSWindowStyleMask,
) -> Retained<NSWindow> {
    // SAFETY: standard AppKit construction on the main thread.
    let w = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            rect,
            mask,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: we keep the Retained alive for the process lifetime.
    unsafe { w.setReleasedWhenClosed(false) };
    w.setTitle(&NSString::from_str(title));
    w.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
        color.0, color.1, color.2, 1.0,
    )));
    w
}

fn raw(w: &NSWindow) -> RawWindow {
    let view = w.contentView().expect("content view");
    let ptr = NonNull::new(Retained::as_ptr(&view) as *mut c_void).unwrap();
    RawWindow::new(RawWindowHandle::AppKit(AppKitWindowHandle::new(ptr)))
}

pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ctl_path = args.get(1).cloned().unwrap_or_else(|| "fixture.ctl".into());
    if let Some(l) = args.get(2) {
        std::env::set_var("LOWK_FIXTURE_LOG", l);
    }
    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let overlay_win = make_window(
        mtm,
        NSRect::new(NSPoint::new(100.0, 500.0), NSSize::new(360.0, 160.0)),
        (1.0, 0.0, 1.0),
        "OVERLAY",
        NSWindowStyleMask::Borderless,
    );
    let main_win = make_window(
        mtm,
        NSRect::new(NSPoint::new(600.0, 300.0), NSSize::new(500.0, 400.0)),
        (0.0, 1.0, 0.0),
        "Lowk Fixture MAIN",
        NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable,
    );
    // Fixture only: keep MAIN above other apps' windows so the probe can see it.
    main_win.setLevel(3);
    // An editable text field in the overlay: the typing (S-F) target.
    let field = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
    field.setFrame(NSRect::new(NSPoint::new(10.0, 10.0), NSSize::new(340.0, 24.0)));
    overlay_win.contentView().unwrap().addSubview(&field);

    let sink = Arc::new(CollectEventSink::default());
    let tone_log: Box<dyn Fn(Tone) + Send> = Box::new(|t| log(format!("TONE {t:?}")));
    let st = create_stealth(
        RawWindows {
            overlay: Some(raw(&overlay_win)),
            main: Some(raw(&main_win)),
            pill: None,
            area: None,
        },
        Arc::new(Main),
        StealthConfig::default(),
        tone_log,
    )
    .expect("create_stealth");
    st.set_event_sink(sink.clone());
    log(format!("fixture ready, status={:?}", st.controller.status()));

    // Initial show happens on the control thread so the run loop is live.
    let st = Arc::new(st);
    let (ov_ptr, field_ptr) = (
        Retained::as_ptr(&overlay_win) as usize,
        Retained::as_ptr(&field) as usize,
    );
    let st2 = st.clone();
    std::thread::spawn(move || {
        let _ = std::fs::remove_file(&ctl_path);
        let mut quit = false;
        while !quit {
            std::thread::sleep(Duration::from_millis(100));
            let Ok(c) = std::fs::read_to_string(&ctl_path) else {
                continue;
            };
            let _ = std::fs::remove_file(&ctl_path);
            for l in c.lines() {
                let p: Vec<&str> = l.split_whitespace().collect();
                log(format!("CMD {l}"));
                match p.as_slice() {
                    ["overlay-show"] => log(format!("show -> {:?}", st2.controller.show())),
                    ["overlay-hide"] => st2.controller.hide(),
                    ["main-show"] => log(format!("main-show -> {:?}", st2.guard.show(MAIN))),
                    ["main-hide"] => st2.guard.hide(MAIN),
                    ["panic"] => st2.controller.panic_hide(),
                    ["overlay-move", x, y] => st2
                        .overlay
                        .set_origin(x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0)),
                    ["mode", m] => {
                        let r = match *m {
                            "passive" => st2.controller.set_mode(OverlayMode::Passive),
                            "interactive" => st2.controller.set_mode(OverlayMode::Interactive),
                            _ => {
                                st2.overlay.escape();
                                Ok(())
                            }
                        };
                        log(format!("mode {m} -> {r:?} now={:?}", st2.controller.mode()));
                    }
                    ["magnet", a, b, c, d] => {
                        let f: Vec<f64> = [a, b, c, d].iter().map(|v| v.parse().unwrap_or(0.0)).collect();
                        let drag = Arc::new(Drag {
                            from: (f[0], f[1]),
                            to: (f[2], f[3]),
                            t0: std::time::Instant::now(),
                        });
                        st2.overlay.move_hold_with(Phase::Down, drag);
                        log(format!("magnet down: mode={:?}", st2.controller.mode()));
                        std::thread::sleep(Duration::from_millis(600));
                        log(format!("magnet mid-release frame={:?}", st2.overlay.frame()));
                        st2.overlay
                            .move_hold_with(Phase::Up, Arc::new(FixedCursor(0.0, 0.0)));
                        log(format!(
                            "magnet up: mode={:?} frame={:?}",
                            st2.controller.mode(),
                            st2.overlay.frame()
                        ));
                    }
                    ["exclusion", on] => {
                        let pol = WindowPolicy {
                            capture_excluded: *on == "on",
                            ..WindowPolicy::OVERLAY
                        };
                        log(format!(
                            "exclusion {on} -> {:?}",
                            st2.guard.set_policy(OVERLAY, pol)
                        ));
                    }
                    ["hide-main", on] => {
                        log(format!("hide-main {on} -> {:?}", st2.set_hide_main(*on == "on")))
                    }
                    ["state"] | ["selftest"] => {
                        log(format!(
                            "status={:?} mode={:?} frame={:?}",
                            st2.controller.status(),
                            st2.controller.mode(),
                            st2.overlay.frame()
                        ));
                        log(format!("selftest={:?}", st2.controller.selftest()));
                        let (o, f) = (ov_ptr, field_ptr);
                        Main.run(Box::new(move || {
                            // SAFETY: pointers outlive the process main; read on the main thread.
                            let w = unsafe { &*(o as *const NSWindow) };
                            let fl = unsafe { &*(f as *const NSTextField) };
                            let app = NSApplication::sharedApplication(unsafe { MainThreadMarker::new_unchecked() });
                            let fr = w.firstResponder().map(|r| format!("{:?}", r.class()));
                            log(format!(
                                "STATE overlay.isKey={} app.isActive={} firstResponder={:?} text='{}' ignoresMouse={} frontmost={:?}",
                                w.isKeyWindow(),
                                app.isActive(),
                                fr,
                                fl.stringValue(),
                                w.ignoresMouseEvents(),
                                NSWorkspace::sharedWorkspace().frontmostApplication().and_then(|a| a.localizedName()).map(|n| n.to_string())
                            ));
                        }));
                    }
                    ["quit"] => {
                        quit = true;
                    }
                    _ => log(format!("unknown cmd {l}")),
                }
            }
        }
        std::process::exit(0);
    });
    // Show both windows before the run loop (apply -> verify -> show).
    log(format!("initial overlay show -> {:?}", st.controller.show()));
    log(format!("initial main show -> {:?}", st.guard.show(MAIN)));
    app.run();
}

/// Cursor that moves linearly from `from` to `to` over 400 ms (held there after).
struct Drag {
    from: (f64, f64),
    to: (f64, f64),
    t0: std::time::Instant,
}
impl CursorSource for Drag {
    fn position(&self) -> (f64, f64) {
        let t = (self.t0.elapsed().as_secs_f64() / 0.4).min(1.0);
        (
            self.from.0 + (self.to.0 - self.from.0) * t,
            self.from.1 + (self.to.1 - self.from.1) * t,
        )
    }
}
