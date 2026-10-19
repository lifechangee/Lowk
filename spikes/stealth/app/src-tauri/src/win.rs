// Windows path. Type-checked only on macOS host; must be run on a real Windows machine.
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::WebviewWindow;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

static EXCLUDE: AtomicBool = AtomicBool::new(true);
pub fn set_excluded_default(on: bool) { EXCLUDE.store(on, Ordering::SeqCst); }

fn hwnd(w: &WebviewWindow) -> HWND { HWND(w.hwnd().expect("hwnd").0 as _) }

pub fn setup_overlay(w: &WebviewWindow) {
    let h = hwnd(w);
    unsafe {
        let mut ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        ex |= (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize;
        ex &= !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex);
        let r = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        crate::log(format!("setup_overlay exstyle=0x{:x} SetWindowPos={:?}", GetWindowLongPtrW(h, GWL_EXSTYLE), r));
    }
}

pub fn after_show(w: &WebviewWindow) {
    unsafe {
        let _ = SetWindowPos(hwnd(w), Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

pub fn apply_exclusion(w: &WebviewWindow) -> String {
    let h = hwnd(w);
    let want = if EXCLUDE.load(Ordering::SeqCst) { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
    unsafe {
        let set = SetWindowDisplayAffinity(h, want);
        let mut got: u32 = 0xdead;
        let get = GetWindowDisplayAffinity(h, &mut got);
        let ok = set.is_ok() && get.is_ok() && got == want.0;
        format!("SetWindowDisplayAffinity={:?} GetWindowDisplayAffinity={:?} affinity=0x{:x} want=0x{:x} ok={}", set, get, got, want.0, ok)
    }
}

// Not needed on Windows (no Spaces); virtual-desktop switches are covered by the 1 s watchdog.
pub fn observe_space_changes(_h: tauri::AppHandle, _f: fn(&tauri::AppHandle)) {}
