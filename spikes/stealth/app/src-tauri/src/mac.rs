use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::*;
use objc2_foundation::NSOperationQueue;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, WebviewWindow};

extern "C" {
    fn object_setClass(obj: *mut c_void, cls: *const c_void) -> *const c_void;
}

static EXCLUDE: AtomicBool = AtomicBool::new(true);
pub fn set_excluded_default(on: bool) { EXCLUDE.store(on, Ordering::SeqCst); }

fn nswin(w: &WebviewWindow) -> &NSWindow {
    unsafe { &*(w.ns_window().expect("ns_window") as *const NSWindow) }
}

pub fn setup_overlay(w: &WebviewWindow) {
    let win = nswin(w);
    unsafe {
        // Turn the tao NSWindow into an NSPanel so .nonactivatingPanel is honored.
        let old = (win as *const NSWindow as *mut AnyObject).as_ref().unwrap().class().name().to_string_lossy().into_owned();
        object_setClass(win as *const NSWindow as *mut c_void, objc2::class!(NSPanel) as *const _ as *const c_void);
        crate::log(format!("overlay class {old} -> NSPanel"));
    }
    let panel: &NSPanel = unsafe { &*(win as *const NSWindow as *const NSPanel) };
    let mask = win.styleMask() | NSWindowStyleMask::NonactivatingPanel;
    win.setStyleMask(mask);
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setHidesOnDeactivate(false);
    win.setLevel(1000); // NSScreenSaverWindowLevel
    win.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    win.setOpaque(false);
    win.setHasShadow(false);
}

pub fn after_show(w: &WebviewWindow) {
    // show/hide can reset these; re-assert everything (found empirically: panic hide->show lost fullscreen-space visibility)
    let win = nswin(w);
    let panel: &NSPanel = unsafe { &*(win as *const NSWindow as *const NSPanel) };
    win.setStyleMask(win.styleMask() | NSWindowStyleMask::NonactivatingPanel);
    panel.setFloatingPanel(true);
    panel.setHidesOnDeactivate(false);
    win.setLevel(1000);
    win.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    win.orderFrontRegardless();
    crate::log(format!("after_show collectionBehavior={} mask={:#x}", win.collectionBehavior().0, win.styleMask().0));
}

pub fn apply_exclusion(w: &WebviewWindow) -> String {
    let win = nswin(w);
    let want = if EXCLUDE.load(Ordering::SeqCst) { NSWindowSharingType::None } else { NSWindowSharingType::ReadOnly };
    let before = win.sharingType();
    if before != want { win.setSharingType(want); }
    let after = win.sharingType();
    format!("sharingType before={} after={} want={} ok={} level={} visible={} winnum={}",
        before.0, after.0, want.0, after == want, win.level(), win.isVisible(), win.windowNumber())
}

pub fn observe_space_changes(h: AppHandle, f: fn(&AppHandle)) {
    use block2::RcBlock;
    use objc2_foundation::NSNotification;
    use std::ptr::NonNull;
    let block = RcBlock::new(move |_n: NonNull<NSNotification>| f(&h));
    unsafe {
        let nc = NSWorkspace::sharedWorkspace().notificationCenter();
        let name = NSWorkspaceActiveSpaceDidChangeNotification;
        let tok: Retained<AnyObject> = Retained::cast_unchecked(nc.addObserverForName_object_queue_usingBlock(
            Some(name), None, Some(&NSOperationQueue::mainQueue()), &block));
        std::mem::forget(tok);
    }
    std::mem::forget(block);
}
