//! Graceful end on SIGTERM / SIGINT / SIGHUP (owner bug A, 2026-10-22: the app "crashed" while the
//! owner was speaking; the system log showed no crash at all, only `SIGTERM` from a tool that
//! pkill'ed the running app). A signal is not a crash: log it loudly (so the next report says
//! "stopped by signal 15", not "closed by itself"), finish the running call so the transcript is
//! saved, tear the capture down (taps and aggregates are destroyed by the session drop) and exit.
use lowk_session::Session;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

static GOT: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_signal(sig: libc::c_int) {
    // async-signal-safe: one atomic store, the watcher thread does the rest
    GOT.store(sig, Ordering::SeqCst);
}

pub fn name(sig: i32) -> &'static str {
    match sig {
        libc::SIGTERM => "SIGTERM",
        libc::SIGINT => "SIGINT",
        libc::SIGHUP => "SIGHUP",
        _ => "signal",
    }
}

pub fn install(handle: AppHandle, session: Arc<Session>) {
    // SAFETY: `on_signal` only performs an atomic store.
    unsafe {
        for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(s, on_signal as *const () as libc::sighandler_t);
        }
    }
    std::thread::Builder::new()
        .name("lowk-signal".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(100));
            let sig = GOT.load(Ordering::SeqCst);
            if sig == 0 {
                continue;
            }
            tracing::warn!(
                signal = name(sig),
                "stopped by a signal (kill/pkill/launchd), not a crash: saving the call and exiting"
            );
            // a hard stop if finishing hangs
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(8));
                tracing::warn!("exit after signal timed out, leaving now");
                std::process::exit(0);
            });
            let _ = session.stop();
            handle.exit(0);
            return;
        })
        .expect("spawn signal watcher");
}
