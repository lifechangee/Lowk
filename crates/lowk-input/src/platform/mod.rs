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
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
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

/// Pure: bring the tracked modifier slots in line with what the OS reports (`actual`), leaving
/// `skip` (the slot of the key being processed) alone. A lost key-up (hook timeout, focus
/// change, secure desktop) otherwise leaves a modifier «stuck down» and every exact-mod bind dies.
/// Returns true when something changed.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn reconcile_mods(tracked: &mut [bool; 8], actual: [bool; 8], skip: Option<usize>) -> bool {
    let mut changed = false;
    for i in 0..8 {
        if Some(i) != skip && tracked[i] != actual[i] {
            tracked[i] = actual[i];
            changed = true;
        }
    }
    changed
}

/// Pure health decision for the Windows hook thread: a modifier whose OS state disagrees with what the
/// hook delivered for a whole poll interval means a key event never reached us (Windows silently drops a
/// low-level hook whose callback exceeded the timeout), so the hook must be re-installed.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn hook_dropped(tracked: &[bool; 8], actual: &[bool; 8]) -> bool {
    tracked != actual
}

#[cfg(test)]
mod tests {
    #[test]
    fn disagreement_after_a_whole_interval_means_a_dropped_hook() {
        let t = [false; 8];
        let mut a = [false; 8];
        assert!(!super::hook_dropped(&t, &a));
        a[2] = true;
        assert!(super::hook_dropped(&t, &a));
    }

    use super::reconcile_mods;

    #[test]
    fn stuck_modifier_is_released() {
        let mut t = [false, false, true, false, false, false, false, false]; // lctrl «stuck»
        assert!(reconcile_mods(&mut t, [false; 8], None));
        assert_eq!(t, [false; 8]);
    }
    #[test]
    fn missed_modifier_down_is_picked_up() {
        let mut t = [false; 8];
        let mut a = [false; 8];
        a[0] = true;
        assert!(reconcile_mods(&mut t, a, None));
        assert!(t[0]);
    }
    #[test]
    fn processed_key_slot_is_left_alone() {
        let mut t = [false; 8];
        let mut a = [false; 8];
        a[2] = true;
        assert!(!reconcile_mods(&mut t, a, Some(2)));
        assert!(!t[2]);
    }
}
