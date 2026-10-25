//! Windows: WH_KEYBOARD_LL + WH_MOUSE_LL on one message-loop thread. Callbacks
//! only normalise the event and call the engine (hook timeout); swallowing is
//! `return 1`. No permission is needed.
use super::{hook_dropped, reconcile_mods, HookError, HookHandle, HookInfo, PermSnapshot};
use crate::engine::Engine;
use crate::keys::from_win_vk;
use crate::matcher::Raw;
use lowk_types::{Mods, MouseBtn};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{mpsc, Arc, Mutex};
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
const LLMHF_INJECTED: u32 = 0x01;

#[derive(Default)]
struct State {
    engine: Option<Arc<Engine>>,
    /// lshift, rshift, lctrl, rctrl, lalt, ralt, lwin, rwin
    mods: [bool; 8],
    down: Vec<u16>,
}

static STATE: Mutex<State> = Mutex::new(State {
    engine: None,
    mods: [false; 8],
    down: Vec::new(),
});

fn mods_of(m: &[bool; 8]) -> Mods {
    Mods {
        shift: m[0] || m[1],
        ctrl: m[2] || m[3],
        alt: m[4] || m[5],
        meta: m[6] || m[7],
    }
}

fn mod_slot(vk: u16) -> Option<usize> {
    match vk {
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
    if kb.flags.0 & LLKHF_INJECTED != 0 {
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
    let slot = mod_slot(vk);
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
    engine.on_raw(Raw::Key {
        code: from_win_vk(vk),
        down,
        repeat,
    })
}

fn on_mouse(msg: u32, ms: &MSLLHOOKSTRUCT) -> bool {
    if ms.flags & LLMHF_INJECTED != 0 {
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
    let engine = STATE.lock().unwrap_or_else(|e| e.into_inner()).engine.clone();
    engine.is_some_and(|e| e.on_raw(Raw::Mouse { button, down }))
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
        let (mut kb, mut ms) = match install() {
            Ok(h) => h,
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        // Health check: Windows removes a low-level hook silently when a callback is too slow. Modifier
        // state polled from the OS must agree with what the hook delivered; if not, re-install the hooks.
        const HEALTH_TIMER: usize = 0x4c6b;
        // Injected modifiers (remote tools) legitimately disagree with the hook: re-install at most every 10 s.
        let mut last_rehook = std::time::Instant::now() - Duration::from_secs(60);
        SetTimer(None, HEALTH_TIMER, 750, None);
        // Force creation of this thread's message queue before announcing the id.
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
        let _ = tx.send(Ok(GetCurrentThreadId()));
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_TIMER && msg.wParam.0 == HEALTH_TIMER {
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
                    if last_rehook.elapsed() < Duration::from_secs(10) {
                        continue;
                    }
                    last_rehook = std::time::Instant::now();
                    let _ = UnhookWindowsHookEx(kb);
                    let _ = UnhookWindowsHookEx(ms);
                    match install() {
                        Ok((k, m)) => (kb, ms) = (k, m),
                        Err(e) => {
                            tracing::error!(%e, "could not re-install the keyboard hook");
                            break;
                        }
                    }
                }
            }
        }
        let _ = KillTimer(None, HEALTH_TIMER);
        let _ = UnhookWindowsHookEx(kb);
        let _ = UnhookWindowsHookEx(ms);
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
