//! The user's UTC offset, measured once at process start.
//!
//! macOS and Linux only report the local offset while the process is single-threaded, so the app measures it
//! first thing in `main` and stores it here; later readers (call titles, anything shown to the user) use it.

use std::sync::atomic::{AtomicI32, Ordering};

const UNSET: i32 = i32::MIN;
static START_OFFSET_SECS: AtomicI32 = AtomicI32::new(UNSET);

/// Records the local UTC offset in seconds (called once at startup, before any thread is spawned).
pub fn set_local_offset_secs(secs: i32) {
    START_OFFSET_SECS.store(secs, Ordering::Relaxed);
}

/// The local UTC offset measured at startup, if it was recorded.
pub fn local_offset_secs() -> Option<i32> {
    match START_OFFSET_SECS.load(Ordering::Relaxed) {
        UNSET => None,
        s => Some(s),
    }
}
