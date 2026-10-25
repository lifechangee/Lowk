//! macOS permission backends (raw framework FFI + objc2 for AVFoundation).
use super::system_audio_heuristic;
use lowk_types::{PermKind, PermState, Result};
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use std::ffi::{c_char, c_void};
use std::sync::mpsc;
use std::time::Duration;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    // IOHIDRequestType: PostEvent = 0, ListenEvent = 1.
    // IOHIDAccessType: Granted = 0, Denied = 1, Unknown = 2.
    fn IOHIDCheckAccess(request: u32) -> u32;
    fn IOHIDRequestAccess(request: u32) -> bool;
}
const HID_LISTEN_EVENT: u32 = 1;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    fn CFDictionaryCreate(
        alloc: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        n: isize,
        key_cb: *const c_void,
        val_cb: *const c_void,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

/// `AXIsProcessTrustedWithOptions({prompt: true})`: shows the system dialog and, more importantly,
/// LISTS the app in Privacy > Accessibility (without this call the pane has no Lowk row to switch on).
fn accessibility_prompt() -> bool {
    // SAFETY: a one-entry CFDictionary of CF constants, released after the call.
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let vals = [kCFBooleanTrue];
        let d = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            vals.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );
        let r = AXIsProcessTrustedWithOptions(d);
        if !d.is_null() {
            CFRelease(d);
        }
        r
    }
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}
#[link(name = "Foundation", kind = "framework")]
extern "C" {}

fn av_audio_type() -> *mut AnyObject {
    // AVMediaTypeAudio == @"soun"
    let ns: &AnyClass = AnyClass::get(c"NSString").expect("NSString");
    // SAFETY: plain NSString factory with a NUL-terminated UTF-8 literal.
    unsafe { msg_send![ns, stringWithUTF8String: c"soun".as_ptr() as *const c_char] }
}

fn mic_status() -> PermState {
    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else {
        return PermState::Unknown;
    };
    // SAFETY: documented class method returning NSInteger.
    let s: isize = unsafe { msg_send![cls, authorizationStatusForMediaType: av_audio_type()] };
    match s {
        0 => PermState::NotDetermined,
        3 => PermState::Granted,
        1 | 2 => PermState::Denied, // restricted / denied
        _ => PermState::Unknown,
    }
}

fn mic_request() -> PermState {
    let cur = mic_status();
    if cur != PermState::NotDetermined {
        return cur;
    }
    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else {
        return PermState::Unknown;
    };
    let (tx, rx) = mpsc::channel::<bool>();
    let block = block2::RcBlock::new(move |granted: Bool| {
        let _ = tx.send(granted.as_bool());
    });
    // SAFETY: documented class method; the block is copied by the callee.
    unsafe {
        let _: () = msg_send![
            cls,
            requestAccessForMediaType: av_audio_type(),
            completionHandler: &*block
        ];
    }
    match rx.recv_timeout(Duration::from_secs(300)) {
        Ok(true) => PermState::Granted,
        Ok(false) => PermState::Denied,
        Err(_) => mic_status(),
    }
}

pub fn status(k: PermKind) -> PermState {
    match k {
        PermKind::Microphone => mic_status(),
        PermKind::SystemAudio => system_audio_heuristic(),
        // SAFETY: argument-free preflight calls.
        PermKind::ScreenRecording => {
            if unsafe { CGPreflightScreenCaptureAccess() } {
                PermState::Granted
            } else {
                // No tri-state API: "never asked" and "denied" look the same.
                PermState::NotDetermined
            }
        }
        PermKind::InputMonitoring => match unsafe { IOHIDCheckAccess(HID_LISTEN_EVENT) } {
            0 => PermState::Granted,
            1 => PermState::Denied,
            _ => PermState::NotDetermined,
        },
        PermKind::Accessibility => {
            if unsafe { AXIsProcessTrusted() } {
                PermState::Granted
            } else {
                PermState::NotDetermined
            }
        }
    }
}

pub fn request(k: PermKind) -> Result<PermState> {
    match k {
        PermKind::Microphone => Ok(mic_request()),
        // No prompt from here: lowk-app runs lowk-audio's probe tap (see perm/mod.rs).
        PermKind::SystemAudio => Ok(system_audio_heuristic()),
        PermKind::ScreenRecording => {
            if status(k) != PermState::Granted {
                // SAFETY: shows the OS prompt once; returns immediately.
                unsafe { CGRequestScreenCaptureAccess() };
            }
            Ok(status(k))
        }
        PermKind::InputMonitoring => {
            if status(k) != PermState::Granted {
                // SAFETY: shows the OS prompt (or pane hint) for ListenEvent.
                unsafe { IOHIDRequestAccess(HID_LISTEN_EVENT) };
            }
            Ok(status(k))
        }
        // The prompt call is what adds Lowk to the Accessibility list; the pane opens too.
        PermKind::Accessibility => {
            if status(k) != PermState::Granted {
                accessibility_prompt();
                super::open_settings(k);
            }
            Ok(status(k))
        }
    }
}

pub fn open_url(url: &str) {
    let _ = crate::process::command("/usr/bin/open").arg(url).status();
}
