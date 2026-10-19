//! Native hook backends. Each platform exposes the same small API; the rest of
//! the crate is platform independent.
use crate::engine::Engine;
use lowk_types::PermKind;
use std::sync::Arc;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as imp;

#[cfg(not(any(target_os = "macos", windows)))]
mod other;
#[cfg(not(any(target_os = "macos", windows)))]
use other as imp;

/// What the running hook can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookInfo {
    /// Events can be swallowed (active tap / WH_*_LL). False = listen-only.
    pub swallow_capable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookError {
    Permission(PermKind),
    Failed(String),
    Unsupported,
}

/// Permission snapshot. `None` = not applicable on this OS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PermSnapshot {
    pub input_monitoring: Option<bool>,
    pub accessibility: Option<bool>,
}

/// Owns a hook thread; dropping it stops the thread.
pub struct HookHandle {
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl HookHandle {
    pub(crate) fn new(stop: Box<dyn FnOnce() + Send>) -> Self {
        Self { stop: Some(stop) }
    }
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        if let Some(f) = self.stop.take() {
            f();
        }
    }
}

/// Start the keyboard+mouse hook thread feeding `engine`.
/// `want_swallow`: at least one bind swallows events (macOS: needs an active tap).
pub fn start(engine: Arc<Engine>, want_swallow: bool) -> Result<(HookHandle, HookInfo), HookError> {
    imp::start(engine, want_swallow)
}

pub fn permissions() -> PermSnapshot {
    imp::permissions()
}

/// macOS: shows the Input Monitoring prompt once. Elsewhere: true.
pub fn request_input_monitoring() -> bool {
    imp::request_input_monitoring()
}

/// macOS: prompts for Accessibility (needed to swallow events). Elsewhere: true.
pub fn request_accessibility() -> bool {
    imp::request_accessibility()
}

pub fn cursor_position() -> (f64, f64) {
    imp::cursor_position()
}
