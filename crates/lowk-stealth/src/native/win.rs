//! Windows backend (Win32 via the `windows` crate). Every method runs on the
//! UI thread. Compiles on any host (`cargo check --target x86_64-pc-windows-msvc`);
//! runtime-unverified until a real Windows session is available.
//!
//! Recipe (docs/ARCHITECTURE.md 7.2): `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`
//! with the return value checked and read back; `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`
//! (+ `WS_EX_LAYERED | WS_EX_TRANSPARENT` while passive), `WS_EX_APPWINDOW`
//! cleared before first show; `HWND_TOPMOST` re-asserted on foreground changes.
//! Builds older than 19041 reject `WDA_EXCLUDEFROMCAPTURE`; the set call then
//! fails and the guard fails closed (we never fall back to `WDA_MONITOR`,
//! which paints a black box).

use crate::backend::{unavailable, Readback, WindowBackend};
use crate::mode::Interaction;
use crate::policy::WindowPolicy;
use crate::RawWindow;
use lowk_types::{Rect, Result};
use parking_lot::Mutex;
use raw_window_handle::RawWindowHandle;
use std::ffi::c_void;
use std::sync::{Arc, OnceLock};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

struct Win {
    hwnd: isize,
    prev_fg: Mutex<isize>,
}

fn h(v: isize) -> HWND {
    HWND(v as *mut c_void)
}

pub fn make_backend(w: &RawWindow) -> Result<Arc<dyn WindowBackend>> {
    match w.handle() {
        RawWindowHandle::Win32(hh) => Ok(Arc::new(Win {
            hwnd: hh.hwnd.get(),
            prev_fg: Mutex::new(0),
        })),
        _ => Err(unavailable("not a Win32 window handle")),
    }
}

impl Win {
    fn hwnd(&self) -> HWND {
        h(self.hwnd)
    }

    fn ex(&self) -> isize {
        // SAFETY: valid HWND owned by the toolkit.
        unsafe { GetWindowLongPtrW(self.hwnd(), GWL_EXSTYLE) }
    }

    fn set_ex(&self, ex: isize) {
        // SAFETY: as above; FRAMECHANGED makes the style change take effect.
        unsafe {
            SetWindowLongPtrW(self.hwnd(), GWL_EXSTYLE, ex);
            let _ = SetWindowPos(
                self.hwnd(),
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }

    fn topmost(&self) {
        // SAFETY: valid HWND.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd(),
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    fn read_affinity(&self) -> Option<u32> {
        let mut got: u32 = 0xdead;
        // SAFETY: valid HWND, valid out pointer.
        unsafe { GetWindowDisplayAffinity(self.hwnd(), &mut got).ok().map(|_| got) }
    }

    fn readback(&self, p: &WindowPolicy, set: &str) -> Readback {
        let aff = self.read_affinity();
        Readback {
            capture_excluded: aff == Some(WDA_EXCLUDEFROMCAPTURE.0),
            window_server_excluded: None,
            detail: format!(
                "{set} affinity={aff:?} want_excluded={} exstyle=0x{:x}",
                p.capture_excluded,
                self.ex()
            ),
        }
    }
}

const STEALTH_EX: isize = (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize;
const PASSIVE_EX: isize = (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as isize;

impl WindowBackend for Win {
    fn prepare(&self, policy: &WindowPolicy) -> Result<()> {
        if policy.hidden_from_switchers {
            let had_layered = self.ex() & WS_EX_LAYERED.0 as isize != 0;
            let ex = (self.ex() | STEALTH_EX | PASSIVE_EX) & !(WS_EX_APPWINDOW.0 as isize);
            self.set_ex(ex);
            if !had_layered {
                // A layered window stays invisible until attributes are set.
                // SAFETY: valid HWND.
                unsafe {
                    let _ = SetLayeredWindowAttributes(self.hwnd(), Default::default(), 255, LWA_ALPHA);
                }
            }
            self.topmost();
        }
        Ok(())
    }

    fn apply(&self, policy: &WindowPolicy) -> Result<Readback> {
        let want = if policy.capture_excluded {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        };
        // SAFETY: valid HWND. The return value is checked (tao ignores it).
        let set = unsafe { SetWindowDisplayAffinity(self.hwnd(), want) };
        if policy.hidden_from_switchers {
            let ex = self.ex();
            if ex & STEALTH_EX != STEALTH_EX || ex & WS_EX_APPWINDOW.0 as isize != 0 {
                self.set_ex((ex | STEALTH_EX) & !(WS_EX_APPWINDOW.0 as isize));
            }
            self.topmost();
        }
        if let Err(e) = &set {
            if policy.capture_excluded {
                return Err(unavailable(format!(
                    "SetWindowDisplayAffinity failed: {e} (Windows build < 19041?)"
                )));
            }
        }
        Ok(self.readback(policy, &format!("set={set:?}")))
    }

    fn verify(&self, policy: &WindowPolicy) -> Readback {
        self.readback(policy, "verify")
    }

    fn show(&self, policy: &WindowPolicy) -> Result<()> {
        // SAFETY: valid HWND.
        unsafe {
            let _ = ShowWindow(
                self.hwnd(),
                if policy.activates {
                    SW_SHOW
                } else {
                    SW_SHOWNOACTIVATE
                },
            );
        }
        if policy.hidden_from_switchers {
            self.topmost();
        }
        Ok(())
    }

    fn hide(&self) {
        // SAFETY: valid HWND.
        unsafe {
            let _ = ShowWindow(self.hwnd(), SW_HIDE);
        }
    }

    fn is_visible(&self) -> bool {
        // SAFETY: valid HWND.
        unsafe { IsWindowVisible(self.hwnd()).as_bool() }
    }

    fn set_interaction(&self, i: Interaction) -> Result<()> {
        // SAFETY: valid HWNDs; all calls on the UI thread.
        unsafe {
            match i {
                Interaction::Interactive => {
                    *self.prev_fg.lock() = GetForegroundWindow().0 as isize;
                    self.set_ex(self.ex() & !((WS_EX_NOACTIVATE.0 | WS_EX_TRANSPARENT.0) as isize));
                    let _ = SetForegroundWindow(self.hwnd());
                    if GetForegroundWindow() != self.hwnd() {
                        // Allowed right after a global-hotkey event; otherwise borrow the
                        // foreground thread's input state.
                        let fg = GetForegroundWindow();
                        let fg_thread = GetWindowThreadProcessId(fg, None);
                        let me = GetCurrentThreadId();
                        let _ = AttachThreadInput(me, fg_thread, true);
                        let _ = SetForegroundWindow(self.hwnd());
                        let _ = AttachThreadInput(me, fg_thread, false);
                    }
                    let _ = SetFocus(Some(self.hwnd()));
                    if GetForegroundWindow() != self.hwnd() {
                        self.set_ex(self.ex() | PASSIVE_EX | WS_EX_NOACTIVATE.0 as isize);
                        return Err(unavailable("overlay could not take the foreground"));
                    }
                }
                Interaction::Passive => {
                    self.set_ex(self.ex() | PASSIVE_EX | WS_EX_NOACTIVATE.0 as isize);
                    let prev = h(std::mem::take(&mut *self.prev_fg.lock()));
                    if !prev.0.is_null()
                        && prev != self.hwnd()
                        && IsWindow(Some(prev)).as_bool()
                        && GetForegroundWindow() == self.hwnd()
                    {
                        let _ = SetForegroundWindow(prev);
                    }
                }
            }
        }
        Ok(())
    }

    fn frame(&self) -> Rect {
        let mut r = RECT::default();
        // SAFETY: valid HWND, valid out pointer.
        unsafe {
            let _ = GetWindowRect(self.hwnd(), &mut r);
        }
        Rect {
            x: r.left as f64,
            y: r.top as f64,
            w: (r.right - r.left) as f64,
            h: (r.bottom - r.top) as f64,
        }
    }

    fn set_origin(&self, x: f64, y: f64) {
        // SAFETY: valid HWND.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd(),
                None,
                x.round() as i32,
                y.round() as i32,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn display_frames(&self) -> Vec<Rect> {
        unsafe extern "system" fn cb(m: HMONITOR, _dc: HDC, _r: *mut RECT, data: LPARAM) -> BOOL {
            // SAFETY: `data` is the `&mut Vec<Rect>` passed below.
            let out = unsafe { &mut *(data.0 as *mut Vec<Rect>) };
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            // SAFETY: valid monitor handle and struct.
            if unsafe { GetMonitorInfoW(m, &mut mi) }.as_bool() {
                let w = mi.rcWork;
                out.push(Rect {
                    x: w.left as f64,
                    y: w.top as f64,
                    w: (w.right - w.left) as f64,
                    h: (w.bottom - w.top) as f64,
                });
            }
            BOOL(1)
        }
        let mut v: Vec<Rect> = Vec::new();
        // SAFETY: callback only runs during the call; pointer is to a live local.
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut v as *mut Vec<Rect> as isize));
        }
        v
    }

    fn is_key(&self) -> bool {
        // SAFETY: no preconditions.
        unsafe { GetForegroundWindow() == self.hwnd() }
    }
}

static ON_CHANGE: OnceLock<Arc<dyn Fn() + Send + Sync>> = OnceLock::new();

unsafe extern "system" fn win_event(
    _hook: HWINEVENTHOOK,
    _ev: u32,
    _hwnd: HWND,
    _obj: i32,
    _child: i32,
    _thr: u32,
    _time: u32,
) {
    if let Some(f) = ON_CHANGE.get() {
        f();
    }
}

/// Other topmost windows (Zoom toolbar) reorder on foreground changes: re-assert.
/// Must be called on a thread that pumps messages (the UI thread). Display
/// changes (`WM_DISPLAYCHANGE`) are forwarded by the app via `on_window_event`.
pub fn install_observers(on_change: Arc<dyn Fn() + Send + Sync>) {
    if ON_CHANGE.set(on_change).is_err() {
        return;
    }
    // SAFETY: out-of-context hook; callback is a plain extern fn.
    unsafe {
        let _ = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
    }
}
