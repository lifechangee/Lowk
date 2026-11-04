//! Windows: WH_KEYBOARD_LL + WH_MOUSE_LL on one message-loop thread. Callbacks
//! only normalise the event and call the engine (hook timeout); swallowing is
//! `return 1`. No permission is needed.
use super::{
    clear_rehook_request, confirm_stale, hook_dropped, reconcile_mods, rehook_due, rehook_requested,
    take_resync, HookError, HookHandle, HookInfo, PermSnapshot,
};
use crate::engine::Engine;
use crate::keys::{from_win_vk, to_win_vk};
use crate::matcher::Raw;
use lowk_types::{Mods, MouseBtn};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::Duration;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetCursorPos, GetMessageW, KillTimer, PeekMessageW, PostThreadMessageW, SetTimer,
    SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_QUIT, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_TIMER, WM_USER, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

const LLKHF_INJECTED: u32 = 0x10;
const LLKHF_EXTENDED: u32 = 0x01;
const LLMHF_INJECTED: u32 = 0x01;

/// Injected input (SendInput, remote tools) is ignored for users. A qa build accepts it only when the run
/// opts in with `LOWK_QA_ACCEPT_INJECTED=1`, so scripts can drive modifier-only binds; read once.
fn injected_setting(feature_on: bool, env: Option<&str>) -> bool {
    feature_on && env == Some("1")
}

fn accept_injected() -> bool {
    static ACCEPT: OnceLock<bool> = OnceLock::new();
    *ACCEPT.get_or_init(|| {
        injected_setting(
            cfg!(feature = "qa"),
            std::env::var("LOWK_QA_ACCEPT_INJECTED").ok().as_deref(),
        )
    })
}

#[derive(Default)]
struct State {
    engine: Option<Arc<Engine>>,
    /// lshift, rshift, lctrl, rctrl, lalt, ralt, lwin, rwin
    mods: [bool; 8],
    down: Vec<u16>,
    /// Inputs whose down was swallowed. Windows does not expose their physical state through
    /// GetAsyncKeyState, so the watchdog must not infer a release from that API.
    swallowed_keys: Vec<u16>,
    /// Mouse buttons (virtual-key, button) believed down.
    btns: Vec<(u16, MouseBtn)>,
    swallowed_btns: Vec<u16>,
}

static STATE: Mutex<State> = Mutex::new(State {
    engine: None,
    mods: [false; 8],
    down: Vec::new(),
    swallowed_keys: Vec::new(),
    btns: Vec::new(),
    swallowed_btns: Vec::new(),
});

/// Thread id of the hook thread (0 = not running), so a mode change can wake it.
static HOOK_TID: AtomicU32 = AtomicU32::new(0);
const WM_RESYNC: u32 = WM_USER + 1;

/// Wakes the hook thread to re-check its key state against the OS now (overlay mode changed).
pub fn wake() {
    let tid = HOOK_TID.load(Ordering::SeqCst);
    if tid != 0 {
        // SAFETY: plain thread message; a stale id only fails.
        unsafe {
            let _ = PostThreadMessageW(tid, WM_RESYNC, WPARAM(0), LPARAM(0));
        }
    }
}

fn key_is_down(vk: u16) -> bool {
    // SAFETY: plain query.
    let state = unsafe { GetAsyncKeyState(vk as i32) };
    state as u16 & 0x8000 != 0
}

fn button_vk(b: MouseBtn) -> u16 {
    match b {
        MouseBtn::Back => 0x05,
        MouseBtn::Forward => 0x06,
        _ => 0x04,
    }
}

/// Keys to check against the OS: the hook's own list plus what the matcher believes is held. The matcher
/// is what blocks modifier-only binds (they fire only with no key held), and its set can outlive the
/// hook's (a key-up handled while the two disagreed): checking only the hook's list then left a key held
/// in the matcher for good, silently (field report: Ctrl / Alt holds dead after Ctrl+\, combos fine, until
/// `\` was pressed again). macOS checks the matcher's set too.
fn tracked_keys(hook_down: &[u16], matcher_held: &[&str]) -> Vec<u16> {
    let mut out = hook_down.to_vec();
    for vk in matcher_held.iter().filter_map(|c| to_win_vk(c)) {
        if !out.contains(&vk) {
            out.push(vk);
        }
    }
    out
}

fn repairable<T: Copy + PartialEq>(tracked: &[T], swallowed: &[T]) -> Vec<T> {
    tracked
        .iter()
        .copied()
        .filter(|k| !swallowed.contains(k))
        .collect()
}

/// A modifier held while the matcher believes a key is down (so modifier-only binds cannot fire) for this
/// long is reported, once per episode.
const BLOCKED_REPORT_AFTER: Duration = Duration::from_secs(2);

/// One «modifier-only binds blocked by a held key» episode: when it started, whether it was reported.
#[derive(Default)]
struct BlockedEpisode {
    since: Option<std::time::Instant>,
    reported: bool,
}

impl BlockedEpisode {
    /// True exactly once per episode, when it has lasted `BLOCKED_REPORT_AFTER`.
    fn tick(&mut self, blocked: bool, now: std::time::Instant) -> bool {
        if !blocked {
            *self = Self::default();
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if self.reported || now.duration_since(since) < BLOCKED_REPORT_AFTER {
            return false;
        }
        self.reported = true;
        true
    }
}

/// Releases keys / buttons whose key-up never reached the hook (see `confirm_stale`), telling the engine so
/// the held binds end. Returns how many were released.
fn heal_stuck_input(
    sus_keys: &mut Vec<u16>,
    sus_btns: &mut Vec<u16>,
    blocked: &mut BlockedEpisode,
    immediate: bool,
) -> usize {
    let Some(engine) = STATE.lock().unwrap_or_else(|e| e.into_inner()).engine.clone() else {
        return 0;
    };
    // The engine lock is released before STATE is taken: no nesting here.
    let (matcher_held, _) = engine.held();
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let tracked = tracked_keys(&st.down, &matcher_held);
    let matcher_only: Vec<u16> = tracked.iter().copied().filter(|k| !st.down.contains(k)).collect();
    // Counts only, never which keys: nothing here may read like a keystroke log.
    let held = matcher_held.len();
    if blocked.tick(
        held > 0 && os_mods().iter().any(|m| *m),
        std::time::Instant::now(),
    ) {
        tracing::warn!(
            held,
            matcher_only = matcher_only.len(),
            os_down = tracked.iter().filter(|k| key_is_down(**k)).count(),
            "input hook: a modifier is held but a key the matcher believes down blocks modifier-only binds"
        );
    }
    let candidates = repairable(&tracked, &st.swallowed_keys);
    let keys = confirm_stale(sus_keys, &candidates, key_is_down, immediate);
    st.down.retain(|k| !keys.contains(k));
    let released_matcher_only = keys.iter().filter(|k| matcher_only.contains(k)).count();
    if released_matcher_only > 0 {
        tracing::info!(
            keys = released_matcher_only,
            "input hook: released a key held only in the matcher (the hook had already seen it go up)"
        );
    }
    let btn_vks: Vec<u16> = st.btns.iter().map(|(vk, _)| *vk).collect();
    let candidates = repairable(&btn_vks, &st.swallowed_btns);
    let vks = confirm_stale(sus_btns, &candidates, key_is_down, immediate);
    let mut btns = vec![];
    st.btns.retain(|(vk, b)| {
        let stale = vks.contains(vk);
        if stale {
            btns.push(*b);
        }
        !stale
    });
    st.swallowed_btns.retain(|vk| !vks.contains(vk));
    drop(st);
    for vk in &keys {
        engine.on_raw(Raw::Key {
            code: from_win_vk(*vk),
            down: false,
            repeat: false,
        });
    }
    for b in &btns {
        engine.on_raw(Raw::Mouse {
            button: *b,
            down: false,
        });
    }
    let n = keys.len() + btns.len();
    if n > 0 {
        // Names no key: only that a key-up went missing and was made up.
        tracing::warn!(
            keys = keys.len(),
            buttons = btns.len(),
            "input hook: a key-up never arrived; released the stuck input (modifier-only binds were blocked by it)"
        );
    }
    n
}

/// Nothing is held: the hooks can be re-registered without losing a key-up.
fn input_idle() -> bool {
    let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    !st.mods.iter().any(|m| *m) && st.down.is_empty() && st.btns.is_empty()
}

fn mods_of(m: &[bool; 8]) -> Mods {
    Mods {
        shift: m[0] || m[1],
        ctrl: m[2] || m[3],
        alt: m[4] || m[5],
        meta: m[6] || m[7],
    }
}

/// Slot in the modifier table for a key event. Besides the sided VKs, some keyboards, remappers and
/// remote-desktop clients deliver the generic VK_SHIFT / VK_CONTROL / VK_MENU: those are sided by the
/// extended flag (right Ctrl / Alt) or the scan code (right Shift). Treating them as ordinary keys
/// would leave a «held key» behind that blocks every modifier-only bind.
fn mod_slot(vk: u16, scan: u32, flags: u32) -> Option<usize> {
    let extended = flags & LLKHF_EXTENDED != 0;
    match vk {
        0x10 => Some(if scan == 0x36 { 1 } else { 0 }),
        0x11 => Some(if extended { 3 } else { 2 }),
        0x12 => Some(if extended { 5 } else { 4 }),
        0xA0 => Some(0),
        0xA1 => Some(1),
        0xA2 => Some(2),
        0xA3 => Some(3),
        0xA4 => Some(4),
        0xA5 => Some(5),
        0x5B => Some(6),
        0x5C => Some(7),
        _ => None,
    }
}

const MOD_VKS: [i32; 8] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C];

fn os_mods() -> [bool; 8] {
    let mut a = [false; 8];
    for (i, vk) in MOD_VKS.iter().enumerate() {
        // SAFETY: plain query.
        a[i] = unsafe { GetAsyncKeyState(*vk) } as u16 & 0x8000 != 0;
    }
    a
}

fn on_key(msg: u32, kb: &KBDLLHOOKSTRUCT) -> bool {
    if kb.flags.0 & LLKHF_INJECTED != 0 && !accept_injected() {
        return false;
    }
    let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
    if !down && !matches!(msg, WM_KEYUP | WM_SYSKEYUP) {
        return false;
    }
    let vk = kb.vkCode as u16;
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(engine) = st.engine.clone() else {
        return false;
    };
    let slot = mod_slot(vk, kb.scanCode, kb.flags.0);
    let before = mods_of(&st.mods);
    if reconcile_mods(&mut st.mods, os_mods(), slot) {
        let m = mods_of(&st.mods);
        tracing::debug!(?before, now = ?m, "modifier state resynced from the OS (lost key-up)");
        if m != before {
            engine.on_raw(Raw::Mods(m));
        }
    }
    if let Some(slot) = slot {
        st.mods[slot] = down;
        let m = mods_of(&st.mods);
        drop(st);
        tracing::debug!(vk, down, mods = ?m, "hook: modifier");
        engine.on_raw(Raw::Mods(m));
        return false; // modifier keys are never swallowed
    }
    let pos = st.down.iter().position(|k| *k == vk);
    let repeat = match (down, pos) {
        (true, Some(_)) => true,
        (true, None) => {
            st.down.push(vk);
            false
        }
        (false, Some(i)) => {
            st.down.swap_remove(i);
            false
        }
        (false, None) => false,
    };
    drop(st);
    let swallowed = engine.on_raw(Raw::Key {
        code: from_win_vk(vk),
        down,
        repeat,
    });
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    st.swallowed_keys.retain(|k| *k != vk);
    if down && swallowed {
        st.swallowed_keys.push(vk);
    }
    swallowed
}

fn on_mouse(msg: u32, ms: &MSLLHOOKSTRUCT) -> bool {
    if ms.flags & LLMHF_INJECTED != 0 && !accept_injected() {
        return false;
    }
    let (button, down) = match msg {
        WM_MBUTTONDOWN => (MouseBtn::Middle, true),
        WM_MBUTTONUP => (MouseBtn::Middle, false),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let b = match (ms.mouseData >> 16) & 0xFFFF {
                1 => MouseBtn::Back,
                2 => MouseBtn::Forward,
                _ => return false,
            };
            (b, msg == WM_XBUTTONDOWN)
        }
        _ => return false,
    };
    let engine = {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let vk = button_vk(button);
        if down {
            if !st.btns.iter().any(|(k, _)| *k == vk) {
                st.btns.push((vk, button));
            }
        } else {
            st.btns.retain(|(k, _)| *k != vk);
        }
        st.engine.clone()
    };
    let swallowed = engine.is_some_and(|e| e.on_raw(Raw::Mouse { button, down }));
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let vk = button_vk(button);
    st.swallowed_btns.retain(|k| *k != vk);
    if down && swallowed {
        st.swallowed_btns.push(vk);
    }
    swallowed
}

unsafe extern "system" fn kb_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == 0 {
        // SAFETY: for WH_KEYBOARD_LL lparam points to a KBDLLHOOKSTRUCT.
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if let Ok(true) = catch_unwind(AssertUnwindSafe(|| on_key(wparam.0 as u32, kb))) {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == 0 {
        // SAFETY: for WH_MOUSE_LL lparam points to an MSLLHOOKSTRUCT.
        let ms = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        if let Ok(true) = catch_unwind(AssertUnwindSafe(|| on_mouse(wparam.0 as u32, ms))) {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn hook_thread(engine: Arc<Engine>, tx: mpsc::Sender<Result<u32, String>>) {
    {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        *st = State {
            engine: Some(engine.clone()),
            ..State::default()
        };
    }
    unsafe {
        let hmod = match GetModuleHandleW(None) {
            Ok(h) => HINSTANCE(h.0),
            Err(e) => {
                let _ = tx.send(Err(e.to_string()));
                return;
            }
        };
        let install = || -> Result<(HHOOK, HHOOK), String> {
            static WARNED: OnceLock<()> = OnceLock::new();
            if accept_injected() && WARNED.set(()).is_ok() {
                tracing::warn!("QA: the input hook accepts injected input");
            }
            let kb =
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), Some(hmod), 0).map_err(|e| e.to_string())?;
            match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hmod), 0) {
                Ok(ms) => Ok((kb, ms)),
                Err(e) => {
                    let _ = UnhookWindowsHookEx(kb);
                    Err(e.to_string())
                }
            }
        };
        let mut hooks: Option<(HHOOK, HHOOK)> = match install() {
            Ok(h) => Some(h),
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        // Watchdog. Windows removes a low-level hook silently when a callback was too slow, and a
        // non-modifier key whose key-up never arrives silences every modifier-only bind. So, every tick:
        //  * modifier state polled from the OS must agree with what the hook delivered, else the hooks are
        //    re-installed (a modifier is held, so only every 10 s);
        //  * keys / buttons believed down while the OS says they are up are released;
        //  * the hooks are re-registered every 30 s while nothing is held, and after an overlay mode change.
        const HEALTH_TIMER: usize = 0x4c6b;
        let mut last_rehook = std::time::Instant::now();
        let mut last_mod_rehook = std::time::Instant::now() - Duration::from_secs(60);
        let (mut sus_keys, mut sus_btns): (Vec<u16>, Vec<u16>) = (vec![], vec![]);
        let mut blocked = BlockedEpisode::default();
        let reinstall = |hooks: &mut Option<(HHOOK, HHOOK)>| {
            if let Some((k, m)) = hooks.take() {
                let _ = UnhookWindowsHookEx(k);
                let _ = UnhookWindowsHookEx(m);
            }
            match install() {
                Ok(h) => *hooks = Some(h),
                // Not fatal: the next tick tries again (the thread used to end here, for good).
                Err(e) => tracing::error!(%e, "could not re-install the input hooks; will retry"),
            }
        };
        SetTimer(None, HEALTH_TIMER, 750, None);
        // Force creation of this thread's message queue before announcing the id.
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
        let tid = GetCurrentThreadId();
        HOOK_TID.store(tid, Ordering::SeqCst);
        let _ = tx.send(Ok(tid));
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let tick = msg.message == WM_TIMER && msg.wParam.0 == HEALTH_TIMER;
            if !tick && msg.message != WM_RESYNC {
                continue;
            }
            // A mode change: compare with the OS at once (what the user holds right now is true).
            let immediate = !tick && take_resync();
            if hooks.is_none() {
                reinstall(&mut hooks);
                last_rehook = std::time::Instant::now();
            }
            heal_stuck_input(&mut sus_keys, &mut sus_btns, &mut blocked, immediate);
            let actual = os_mods();
            let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
            if hook_dropped(&st.mods, &actual) {
                let before = mods_of(&st.mods);
                reconcile_mods(&mut st.mods, actual, None);
                let now = mods_of(&st.mods);
                let engine = st.engine.clone();
                drop(st);
                tracing::warn!(
                    ?before,
                    ?now,
                    "keyboard hook missed events: modifier state resynced, hooks re-installed"
                );
                if let Some(e) = engine {
                    e.on_raw(Raw::Mods(now));
                }
                // Injected modifiers (remote tools) legitimately disagree with the hook: at most every 10 s.
                if last_mod_rehook.elapsed() >= Duration::from_secs(10) {
                    last_mod_rehook = std::time::Instant::now();
                    last_rehook = std::time::Instant::now();
                    reinstall(&mut hooks);
                }
                continue;
            }
            drop(st);
            let requested = rehook_requested();
            if rehook_due(last_rehook.elapsed(), input_idle(), requested) {
                clear_rehook_request();
                last_rehook = std::time::Instant::now();
                reinstall(&mut hooks);
                if requested {
                    tracing::info!("input hooks re-registered after an overlay mode change");
                } else {
                    tracing::debug!("input hooks re-registered (periodic)");
                }
            }
        }
        HOOK_TID.store(0, Ordering::SeqCst);
        let _ = KillTimer(None, HEALTH_TIMER);
        if let Some((k, m)) = hooks.take() {
            let _ = UnhookWindowsHookEx(k);
            let _ = UnhookWindowsHookEx(m);
        }
    }
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    st.engine = None;
    drop(st);
    engine.on_raw(Raw::Reset);
}

pub fn start(engine: Arc<Engine>, _want_swallow: bool) -> Result<(HookHandle, HookInfo), HookError> {
    let (tx, rx) = mpsc::channel();
    let join = std::thread::Builder::new()
        .name("lowk-input-hooks".into())
        .spawn(move || hook_thread(engine, tx))
        .map_err(|e| HookError::Failed(e.to_string()))?;
    let tid = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => return Err(HookError::Failed(e)),
        Err(_) => return Err(HookError::Failed("hook thread did not start".into())),
    };
    let stop = Box::new(move || {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        let _ = join.join();
    });
    Ok((
        HookHandle::new(stop),
        HookInfo {
            swallow_capable: true,
        },
    ))
}

pub fn permissions() -> PermSnapshot {
    PermSnapshot::default()
}
pub fn request_input_monitoring() -> bool {
    true
}
pub fn request_accessibility() -> bool {
    true
}

pub fn cursor_position() -> (f64, f64) {
    let mut p = POINT::default();
    // SAFETY: plain out-parameter call.
    if unsafe { GetCursorPos(&mut p) }.is_ok() {
        (p.x as f64, p.y as f64)
    } else {
        (0.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn injected_setting_requires_qa_and_exact_opt_in() {
        assert!(!super::injected_setting(false, Some("1")));
        assert!(!super::injected_setting(true, None));
        assert!(!super::injected_setting(true, Some("0")));
        assert!(super::injected_setting(true, Some("1")));
    }

    #[cfg(not(feature = "qa"))]
    #[test]
    fn injected_input_stays_disabled_without_qa_feature() {
        // SAFETY: this test is the only code changing this process environment.
        unsafe { std::env::set_var("LOWK_QA_ACCEPT_INJECTED", "1") };
        assert!(!super::accept_injected());
        // SAFETY: restore the process environment after the assertion.
        unsafe { std::env::remove_var("LOWK_QA_ACCEPT_INJECTED") };
    }

    #[test]
    fn swallowed_holds_are_never_released_from_async_key_state_absence() {
        let tracked = [1u16, 2];
        let swallowed = [1u16];
        let candidates = super::repairable(&tracked, &swallowed);
        let mut suspects = vec![];
        let release = crate::platform::confirm_stale(&mut suspects, &candidates, |_| false, true);
        assert_eq!(release, [2]);
        assert!(tracked.contains(&1));
    }

    #[test]
    fn a_blocked_episode_is_reported_once_after_two_seconds() {
        let t0 = std::time::Instant::now();
        let at = |ms| t0 + std::time::Duration::from_millis(ms);
        let mut e = super::BlockedEpisode::default();
        assert!(!e.tick(true, at(0)));
        assert!(!e.tick(true, at(1999)));
        assert!(e.tick(true, at(2000)));
        assert!(!e.tick(true, at(2750)), "once per episode");
        assert!(!e.tick(false, at(3500)));
        assert!(!e.tick(true, at(4000)), "a new episode starts its own clock");
        assert!(e.tick(true, at(6000)));
    }

    #[test]
    fn generic_modifier_vks_are_modifiers_not_held_keys() {
        // A generic VK_CONTROL down/up must land in the modifier table, never in the held-key list.
        assert_eq!(super::mod_slot(0x11, 0x1D, 0), Some(2));
        assert_eq!(super::mod_slot(0x11, 0x1D, 0x01), Some(3));
        assert_eq!(super::mod_slot(0x12, 0x38, 0), Some(4));
        assert_eq!(super::mod_slot(0x12, 0x38, 0x01), Some(5));
        assert_eq!(super::mod_slot(0x10, 0x2A, 0), Some(0));
        assert_eq!(super::mod_slot(0x10, 0x36, 0), Some(1));
        assert_eq!(super::mod_slot(0xA2, 0x1D, 0), Some(2));
        assert_eq!(super::mod_slot(0x41, 0x1E, 0), None);
        let mut mods = [false; 8];
        for down in [true, false] {
            let slot = super::mod_slot(0x11, 0x1D, 0).unwrap();
            mods[slot] = down;
            assert_eq!(super::mods_of(&mods).ctrl, down);
        }
    }

    #[test]
    fn a_key_held_only_in_the_matcher_is_checked_against_the_os() {
        // The hook's list lost `\` but the matcher still holds it: it must be a repair candidate, or
        // every modifier-only bind stays blocked (field report, Ctrl+\).
        let tracked = super::tracked_keys(&[0x41], &["Backslash", "KeyA"]);
        assert_eq!(tracked, [0x41, 0xDC]);
        let mut suspects = vec![];
        let release = crate::platform::confirm_stale(&mut suspects, &tracked, |_| false, true);
        assert_eq!(release, [0x41, 0xDC]);
    }
}
