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
pub(crate) mod win;
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

/// What the running hook can do RIGHT NOW. The macOS supervisor re-creates the tap on its own and the
/// new tap may be listen-only (Accessibility lost) or absent (Input Monitoring revoked); it publishes
/// that here so status and reports do not keep claiming what was true at start.
#[derive(Debug)]
pub struct LiveInfo {
    running: std::sync::atomic::AtomicBool,
    swallow: std::sync::atomic::AtomicBool,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl LiveInfo {
    pub(crate) fn new(running: bool, swallow: bool) -> Self {
        use std::sync::atomic::AtomicBool;
        Self {
            running: AtomicBool::new(running),
            swallow: AtomicBool::new(swallow),
        }
    }
    pub(crate) fn publish(&self, running: bool, swallow: bool) {
        use std::sync::atomic::Ordering::SeqCst;
        self.running.store(running, SeqCst);
        self.swallow.store(swallow, SeqCst);
    }
    /// `None`: the hook thread is alive but has no working tap at the moment.
    pub fn current(&self) -> Option<HookInfo> {
        use std::sync::atomic::Ordering::SeqCst;
        self.running.load(SeqCst).then(|| HookInfo {
            swallow_capable: self.swallow.load(SeqCst),
        })
    }
}

/// Owns a hook thread; dropping it stops the thread.
pub struct HookHandle {
    stop: Option<Box<dyn FnOnce() + Send>>,
    live: Option<std::sync::Arc<LiveInfo>>,
}

impl HookHandle {
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub(crate) fn new(stop: Box<dyn FnOnce() + Send>) -> Self {
        Self {
            stop: Some(stop),
            live: None,
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn with_live(mut self, live: std::sync::Arc<LiveInfo>) -> Self {
        self.live = Some(live);
        self
    }

    /// Live capability, when the backend publishes it.
    pub fn live(&self) -> Option<std::sync::Arc<LiveInfo>> {
        self.live.clone()
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

/// Pure: which tracked keys/buttons are «stuck down» because their key-up never reached the hook
/// (a hook timeout, an elevated or secure window in front, an injected key-up we ignore, a focus
/// change at the wrong moment). A stuck non-modifier key silences EVERY modifier-only bind (click-hold,
/// move-hold: they only fire while no other key is down) until the same key is pressed and released
/// again, which is why toggling the overlay (a key combo) «repaired» the binds.
///
/// `suspects` carries the candidates of the previous check: unless `immediate`, a key must look released
/// on two checks in a row (a key pressed this very instant may not be in the OS state yet). Returns the
/// ones to release now.
#[cfg_attr(not(any(windows, target_os = "macos", test)), allow(dead_code))]
pub(crate) fn confirm_stale<T: Copy + PartialEq>(
    suspects: &mut Vec<T>,
    tracked: &[T],
    os_down: impl Fn(T) -> bool,
    immediate: bool,
) -> Vec<T> {
    let now: Vec<T> = tracked.iter().copied().filter(|k| !os_down(*k)).collect();
    let release: Vec<T> = if immediate {
        now.clone()
    } else {
        now.iter().copied().filter(|k| suspects.contains(k)).collect()
    };
    *suspects = now.into_iter().filter(|k| !release.contains(k)).collect();
    release
}

/// How often the Windows hooks are re-registered while idle, and the shortest gap after a request
/// (an overlay mode change). Windows removes a low-level hook silently when a callback was too slow and
/// never tells the owner; a hook cannot be probed without injecting input, so it is simply put in again.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const REHOOK_PERIODIC: std::time::Duration = std::time::Duration::from_secs(30);
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const REHOOK_MIN_GAP: std::time::Duration = std::time::Duration::from_secs(5);

/// Pure: re-register the hooks now? Only while nothing is held (a key event lost in the instant between
/// unhook and install would be a lost key-up), every `REHOOK_PERIODIC`, or after a request once
/// `REHOOK_MIN_GAP` has passed.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn rehook_due(since_last: std::time::Duration, idle: bool, requested: bool) -> bool {
    idle && (since_last >= REHOOK_PERIODIC || (requested && since_last >= REHOOK_MIN_GAP))
}

static RESYNC_REQ: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static REHOOK_REQ: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The overlay mode changed (interactive on / off): bring the hook's key state in line with the OS now and
/// re-register the hooks at the next idle moment. Cheap and safe to call often; a no-op off Windows.
pub fn request_resync() {
    use std::sync::atomic::Ordering::SeqCst;
    RESYNC_REQ.store(true, SeqCst);
    REHOOK_REQ.store(true, SeqCst);
    imp::wake();
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub(crate) fn take_resync() -> bool {
    RESYNC_REQ.swap(false, std::sync::atomic::Ordering::SeqCst)
}
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn rehook_requested() -> bool {
    REHOOK_REQ.load(std::sync::atomic::Ordering::SeqCst)
}
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn clear_rehook_request() {
    REHOOK_REQ.store(false, std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_key_whose_up_was_lost_is_released_after_two_looks() {
        let mut sus: Vec<u16> = vec![];
        let tracked = [0x49u16, 0x41]; // I and A believed down; the OS says only A is
        let os = |k: u16| k == 0x41;
        assert!(
            confirm_stale(&mut sus, &tracked, os, false).is_empty(),
            "first look only suspects"
        );
        assert_eq!(confirm_stale(&mut sus, &tracked, os, false), vec![0x49]);
        assert!(sus.is_empty());
        // a key the OS still reports down is never released
        assert!(confirm_stale(&mut sus, &[0x41], os, true).is_empty());
    }
    #[test]
    fn a_key_pressed_between_two_looks_is_not_released() {
        let mut sus: Vec<u16> = vec![];
        let tracked = [0x49u16];
        assert!(confirm_stale(&mut sus, &tracked, |_| false, false).is_empty());
        // the next look finds it down again (released and pressed anew): suspicion is dropped
        assert!(confirm_stale(&mut sus, &tracked, |_| true, false).is_empty());
        assert!(sus.is_empty());
        assert!(confirm_stale(&mut sus, &tracked, |_| false, false).is_empty());
    }
    #[test]
    fn a_resync_request_releases_at_once() {
        let mut sus: Vec<u16> = vec![];
        assert_eq!(confirm_stale(&mut sus, &[0x49u16], |_| false, true), vec![0x49]);
    }
    #[test]
    fn hooks_are_re_registered_only_when_idle_and_due() {
        let s = Duration::from_secs;
        assert!(!rehook_due(s(10), true, false), "not due yet");
        assert!(rehook_due(s(30), true, false), "periodic");
        assert!(!rehook_due(s(60), false, false), "never while a key is held");
        assert!(!rehook_due(s(2), true, true), "requests are rate limited");
        assert!(rehook_due(s(6), true, true), "a mode change asks for one");
        assert!(!rehook_due(s(6), false, true), "and waits for idle");
    }

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
