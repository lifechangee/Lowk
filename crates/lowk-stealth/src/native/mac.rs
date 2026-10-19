//! macOS backend (AppKit via objc2). Every method runs on the main thread.
//!
//! Ported from `spikes/stealth/app/src-tauri/src/mac.rs`. The overlay's class is
//! swapped to `LowkPanel` (an `NSPanel` subclass) so that `.nonactivatingPanel`
//! is honoured and `canBecomeKeyWindow` can be turned on for typing
//! (interactive mode) without activating the app.

use crate::backend::{unavailable, Readback, WindowBackend};
use crate::mode::Interaction;
use crate::policy::{WindowPolicy, WindowRole};
use crate::RawWindow;
use block2::RcBlock;
use lowk_types::{CursorSource, Rect, Result};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{define_class, msg_send, ClassType, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDictionary, NSNotification, NSOperationQueue, NSPoint, NSString,
};
use parking_lot::Mutex;
use raw_window_handle::RawWindowHandle;
use std::collections::HashSet;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, LazyLock};

const SCREEN_SAVER_LEVEL: isize = 1000; // NSScreenSaverWindowLevel

extern "C" {
    fn object_setClass(obj: *mut c_void, cls: *const c_void) -> *const c_void;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *mut c_void;
    fn CGEventCreate(source: *const c_void) -> *mut c_void;
    fn CGEventGetLocation(ev: *const c_void) -> CGPoint;
    fn CFRelease(cf: *const c_void);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

/// Windows (by pointer) currently allowed to become key.
static CAN_KEY: LazyLock<Mutex<HashSet<usize>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

define_class!(
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "LowkPanel"]
    struct LowkPanel;

    unsafe impl NSObjectProtocol for LowkPanel {}

    impl LowkPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            CAN_KEY.lock().contains(&(self as *const Self as usize))
        }
        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            false
        }
    }
);

struct Win {
    win: Retained<NSWindow>,
    prev_front_pid: Mutex<Option<i32>>,
}
// SAFETY: only touched on the main thread (guaranteed by the guard / controller).
unsafe impl Send for Win {}
unsafe impl Sync for Win {}

fn mtm() -> MainThreadMarker {
    // SAFETY: the crate contract is "native calls only on the main thread".
    unsafe { MainThreadMarker::new_unchecked() }
}

pub fn make_backend(w: &RawWindow) -> Result<Arc<dyn WindowBackend>> {
    let win: Retained<NSWindow> = match w.handle() {
        RawWindowHandle::AppKit(h) => {
            // SAFETY: the pointer is a live NSView supplied by the window toolkit.
            let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
            view.window().ok_or_else(|| unavailable("NSView has no window"))?
        }
        _ => return Err(unavailable("not an AppKit window handle")),
    };
    Ok(Arc::new(Win {
        win,
        prev_front_pid: Mutex::new(None),
    }))
}

fn primary_height() -> f64 {
    NSScreen::screens(mtm())
        .firstObject()
        .map(|s| s.frame().size.height)
        .unwrap_or(0.0)
}

impl Win {
    fn panel(&self) -> &NSPanel {
        // SAFETY: only called after `prepare` swapped the class (stealth roles).
        unsafe { &*(Retained::as_ptr(&self.win) as *const NSPanel) }
    }

    fn sharing_state(&self) -> Option<bool> {
        // kCGWindowListOptionIncludingWindow = 1<<3
        let num = self.win.windowNumber() as u32;
        // SAFETY: CF "copy" function returns +1 or null; we release it.
        unsafe {
            let raw = CGWindowListCopyWindowInfo(1 << 3, num);
            if raw.is_null() {
                return None;
            }
            let arr: Retained<NSArray<NSDictionary<NSString, AnyObject>>> = Retained::from_raw(raw.cast())?;
            let d = arr.firstObject()?;
            let v = d.objectForKey(&NSString::from_str("kCGWindowSharingState"))?;
            let n: i32 = msg_send![&*v, intValue];
            Some(n == 0)
        }
    }

    fn assert_stealth_attrs(&self) {
        let w = &self.win;
        w.setLevel(SCREEN_SAVER_LEVEL);
        w.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        w.setStyleMask(w.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        let p = self.panel();
        p.setFloatingPanel(true);
        p.setHidesOnDeactivate(false);
        w.setCanHide(false);
    }

    fn readback(&self, p: &WindowPolicy) -> Readback {
        let st = self.win.sharingType();
        let ws = self.sharing_state();
        Readback {
            capture_excluded: st == NSWindowSharingType::None,
            window_server_excluded: if self.win.isVisible() { ws } else { None },
            detail: format!(
                "sharingType={} want_excluded={} cgSharingExcluded={:?} level={} visible={} winnum={}",
                st.0,
                p.capture_excluded,
                ws,
                self.win.level(),
                self.win.isVisible(),
                self.win.windowNumber()
            ),
        }
    }
}

impl WindowBackend for Win {
    fn prepare(&self, policy: &WindowPolicy) -> Result<()> {
        if policy.hidden_from_switchers {
            let is_panel = self.win.isKindOfClass(NSPanel::class());
            if !is_panel {
                let cls = mtm().alloc::<LowkPanel>();
                let _ = cls; // ensure the class is registered before the swap
                let target = LowkPanel::class();
                // SAFETY: LowkPanel only overrides two methods and adds no ivars,
                // so the instance layout is identical (spike-proven).
                unsafe {
                    object_setClass(
                        Retained::as_ptr(&self.win) as *mut c_void,
                        target as *const _ as *const c_void,
                    );
                }
            }
            self.assert_stealth_attrs();
            let p = self.panel();
            p.setBecomesKeyOnlyIfNeeded(false);
            self.win.setOpaque(false);
            self.win.setHasShadow(false);
            self.win.setIgnoresMouseEvents(true);
        }
        Ok(())
    }

    fn apply(&self, policy: &WindowPolicy) -> Result<Readback> {
        let want = if policy.capture_excluded {
            NSWindowSharingType::None
        } else {
            NSWindowSharingType::ReadOnly
        };
        if self.win.sharingType() != want {
            self.win.setSharingType(want);
        }
        if policy.hidden_from_switchers {
            self.assert_stealth_attrs();
        }
        Ok(self.readback(policy))
    }

    fn verify(&self, policy: &WindowPolicy) -> Readback {
        self.readback(policy)
    }

    fn show(&self, policy: &WindowPolicy) -> Result<()> {
        if policy.role == WindowRole::Main || policy.activates {
            self.win.makeKeyAndOrderFront(None);
        } else {
            self.win.orderFrontRegardless();
        }
        Ok(())
    }

    fn hide(&self) {
        self.win.orderOut(None);
    }

    fn is_visible(&self) -> bool {
        self.win.isVisible()
    }

    fn set_interaction(&self, i: Interaction) -> Result<()> {
        let key = Retained::as_ptr(&self.win) as usize;
        match i {
            Interaction::Interactive => {
                CAN_KEY.lock().insert(key);
                *self.prev_front_pid.lock() = NSWorkspace::sharedWorkspace()
                    .frontmostApplication()
                    .map(|a| a.processIdentifier());
                self.win.setIgnoresMouseEvents(false);
                // Key without activating the app: the underlying app stays frontmost.
                self.win.makeKeyAndOrderFront(None);
                if !self.win.isKeyWindow() {
                    CAN_KEY.lock().remove(&key);
                    self.win.setIgnoresMouseEvents(true);
                    return Err(unavailable("overlay could not become key window"));
                }
            }
            Interaction::Passive => {
                CAN_KEY.lock().remove(&key);
                self.win.setIgnoresMouseEvents(true);
                if self.win.isKeyWindow() {
                    // Dropping key status hands it back to the still-active app below.
                    self.win.orderOut(None);
                    self.win.orderFrontRegardless();
                }
                *self.prev_front_pid.lock() = None;
            }
        }
        Ok(())
    }

    fn frame(&self) -> Rect {
        let f = self.win.frame();
        crate::magnet::flip_y(
            Rect {
                x: f.origin.x,
                y: f.origin.y,
                w: f.size.width,
                h: f.size.height,
            },
            primary_height(),
        )
    }

    fn set_origin(&self, x: f64, y: f64) {
        self.win.setFrameTopLeftPoint(NSPoint {
            x,
            y: primary_height() - y,
        });
    }

    fn display_frames(&self) -> Vec<Rect> {
        let ph = primary_height();
        NSScreen::screens(mtm())
            .iter()
            .map(|s| {
                let f = s.visibleFrame();
                crate::magnet::flip_y(
                    Rect {
                        x: f.origin.x,
                        y: f.origin.y,
                        w: f.size.width,
                        h: f.size.height,
                    },
                    ph,
                )
            })
            .collect()
    }

    fn is_key(&self) -> bool {
        self.win.isKeyWindow()
    }
}

struct MacCursor;
impl CursorSource for MacCursor {
    fn position(&self) -> (f64, f64) {
        // CGEvent location is global, top-left origin, points; needs no permission.
        // SAFETY: CGEventCreate(NULL) returns +1 or null.
        unsafe {
            let ev = CGEventCreate(std::ptr::null());
            if ev.is_null() {
                return (0.0, 0.0);
            }
            let p = CGEventGetLocation(ev);
            CFRelease(ev);
            (p.x, p.y)
        }
    }
}

pub fn cursor() -> Arc<dyn CursorSource> {
    Arc::new(MacCursor)
}

/// Space change, display reconfiguration and wake re-apply everything.
pub fn install_observers(on_change: Arc<dyn Fn() + Send + Sync>) {
    let add = |center: &objc2_foundation::NSNotificationCenter,
               name: &objc2_foundation::NSNotificationName| {
        let cb = on_change.clone();
        let block = RcBlock::new(move |_n: NonNull<NSNotification>| cb());
        // SAFETY: block is valid for the observer's life (leaked: observers live for the process).
        unsafe {
            let tok: Retained<AnyObject> =
                Retained::cast_unchecked(center.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    None,
                    Some(&NSOperationQueue::mainQueue()),
                    &block,
                ));
            std::mem::forget(tok);
        }
        std::mem::forget(block);
    };
    // SAFETY: notification-name statics are valid for the process lifetime.
    unsafe {
        let ws = NSWorkspace::sharedWorkspace().notificationCenter();
        add(&ws, NSWorkspaceActiveSpaceDidChangeNotification);
        add(&ws, NSWorkspaceDidWakeNotification);
        add(
            &objc2_foundation::NSNotificationCenter::defaultCenter(),
            NSApplicationDidChangeScreenParametersNotification,
        );
    }
}
