//! Clocks. All audio frames carry [`MonoNs`] from one process-wide origin.
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::OnceLock;
use std::time::Instant;

/// Nanoseconds since the process-wide monotonic origin (see [`now_mono_ns`]).
/// Fits in 2^53 for ~104 days, so it is a plain JS number in the UI.
pub type MonoNs = u64;

static ORIGIN: OnceLock<Instant> = OnceLock::new();

/// Current monotonic time. Capture backends may substitute device timestamps
/// but must convert them onto this origin.
pub fn now_mono_ns() -> MonoNs {
    ORIGIN.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// Maps monotonic time to call-relative offsets and wall time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionClock {
    pub t0_mono: MonoNs,
    /// Unix milliseconds at `t0_mono`.
    pub t0_wall_ms: i64,
}

impl SessionClock {
    pub fn offset_ns(&self, t: MonoNs) -> u64 {
        t.saturating_sub(self.t0_mono)
    }
    pub fn offset_label(&self, t: MonoNs) -> String {
        fmt_offset(self.offset_ns(t) / 1_000_000_000)
    }
}

/// `[HH:MM:SS]` body for transcript lines (`HHH` beyond 99 h).
pub fn fmt_offset(total_secs: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        total_secs / 3600,
        (total_secs / 60) % 60,
        total_secs % 60
    )
}
