//! macOS: CGEventTap on a dedicated CFRunLoop thread (raw FFI, no extra crates).
//! Input Monitoring is required for the tap; swallowing needs an active tap
//! (Accessibility). Without Accessibility we fall back to a listen-only tap.
use super::{HookError, HookHandle, HookInfo, PermSnapshot};
use crate::engine::Engine;
use crate::keys::from_mac_vk;
use crate::matcher::Raw;
use lowk_types::{Mods, MouseBtn, PermKind};
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

type CFTypeRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

type TapCallback = extern "C" fn(*mut c_void, u32, CFTypeRef, *mut c_void) -> CFTypeRef;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> CFTypeRef;
    fn CGEventTapEnable(tap: CFTypeRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CFTypeRef, field: u32) -> i64;
    fn CGEventGetFlags(event: CFTypeRef) -> u64;
    fn CGEventGetLocation(event: CFTypeRef) -> CGPoint;
    fn CGEventCreate(source: CFTypeRef) -> CFTypeRef;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopCommonModes: CFTypeRef;
    fn CFMachPortCreateRunLoopSource(alloc: CFTypeRef, port: CFTypeRef, order: isize) -> CFTypeRef;
    fn CFMachPortInvalidate(port: CFTypeRef);
    fn CFRunLoopGetCurrent() -> CFTypeRef;
    fn CFRunLoopAddSource(rl: CFTypeRef, source: CFTypeRef, mode: CFTypeRef);
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: CFTypeRef);
    fn CFRelease(cf: CFTypeRef);
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const OTHER_DOWN: u32 = 25;
const OTHER_UP: u32 = 26;
const TAP_DISABLED_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_USER: u32 = 0xFFFF_FFFF;

const FIELD_AUTOREPEAT: u32 = 8;
const FIELD_KEYCODE: u32 = 9;
const FIELD_BUTTON: u32 = 3;

const FLAG_SHIFT: u64 = 0x2_0000;
const FLAG_CTRL: u64 = 0x4_0000;
const FLAG_ALT: u64 = 0x8_0000;
const FLAG_CMD: u64 = 0x10_0000;

fn mods_from_flags(f: u64) -> Mods {
    Mods {
        shift: f & FLAG_SHIFT != 0,
        ctrl: f & FLAG_CTRL != 0,
        alt: f & FLAG_ALT != 0,
        meta: f & FLAG_CMD != 0,
    }
}
fn pack(m: Mods) -> u8 {
    m.shift as u8 | (m.ctrl as u8) << 1 | (m.alt as u8) << 2 | (m.meta as u8) << 3
}

fn button_from_number(n: i64) -> MouseBtn {
    match n {
        2 => MouseBtn::Middle,
        3 => MouseBtn::Back,
        4 => MouseBtn::Forward,
        n => MouseBtn::Other(n.clamp(0, 255) as u8),
    }
}

struct Ctx {
    engine: Arc<Engine>,
    tap: AtomicPtr<c_void>,
    last_mods: AtomicU8,
}

fn handle(ctx: &Ctx, ty: u32, event: CFTypeRef) -> bool {
    if ty == TAP_DISABLED_TIMEOUT || ty == TAP_DISABLED_USER {
        let tap = ctx.tap.load(Ordering::Acquire);
        if !tap.is_null() {
            unsafe { CGEventTapEnable(tap, true) };
        }
        ctx.last_mods.store(0, Ordering::Relaxed);
        ctx.engine.on_raw(Raw::Reset);
        return false;
    }
    // SAFETY: `event` is valid for the duration of the callback.
    let flags = unsafe { CGEventGetFlags(event) };
    let mods = mods_from_flags(flags);
    if ctx.last_mods.swap(pack(mods), Ordering::Relaxed) != pack(mods) {
        ctx.engine.on_raw(Raw::Mods(mods));
    }
    // Only the fields needed for matching are read; key content is never stored.
    match ty {
        KEY_DOWN | KEY_UP => {
            let (vk, rep) = unsafe {
                (
                    CGEventGetIntegerValueField(event, FIELD_KEYCODE),
                    CGEventGetIntegerValueField(event, FIELD_AUTOREPEAT),
                )
            };
            ctx.engine.on_raw(Raw::Key {
                code: from_mac_vk(vk as u16),
                down: ty == KEY_DOWN,
                repeat: rep != 0,
            })
        }
        OTHER_DOWN | OTHER_UP => {
            let n = unsafe { CGEventGetIntegerValueField(event, FIELD_BUTTON) };
            ctx.engine.on_raw(Raw::Mouse {
                button: button_from_number(n),
                down: ty == OTHER_DOWN,
            })
        }
        _ => false,
    }
}

extern "C" fn callback(_proxy: *mut c_void, ty: u32, event: CFTypeRef, info: *mut c_void) -> CFTypeRef {
    // SAFETY: `info` is the leaked Box<Ctx> that outlives the tap.
    let ctx = unsafe { &*(info as *const Ctx) };
    // A panic must never unwind through the C frame.
    match catch_unwind(AssertUnwindSafe(|| handle(ctx, ty, event))) {
        Ok(true) => null_mut(),
        _ => event,
    }
}

struct Started {
    runloop: usize,
    active: bool,
}

fn tap_thread(engine: Arc<Engine>, active: bool, tx: mpsc::Sender<Result<Started, String>>) {
    let ctx = Box::into_raw(Box::new(Ctx {
        engine,
        tap: AtomicPtr::new(null_mut()),
        last_mods: AtomicU8::new(0),
    }));
    let mask: u64 =
        (1 << KEY_DOWN) | (1 << KEY_UP) | (1 << FLAGS_CHANGED) | (1 << OTHER_DOWN) | (1 << OTHER_UP);
    let create = |active: bool| unsafe {
        // session tap (1)? HID tap (0) sees events earliest; head insert (0).
        CGEventTapCreate(
            0,
            0,
            if active { 0 } else { 1 },
            mask,
            callback,
            ctx as *mut c_void,
        )
    };
    let mut is_active = active;
    let mut tap = create(active);
    if tap.is_null() && active {
        is_active = false;
        tap = create(false);
    }
    if tap.is_null() {
        let _ = tx.send(Err("CGEventTapCreate returned null".into()));
        drop(unsafe { Box::from_raw(ctx) });
        return;
    }
    unsafe {
        (*ctx).tap.store(tap, Ordering::Release);
        let src = CFMachPortCreateRunLoopSource(null_mut(), tap, 0);
        let rl = CFRunLoopGetCurrent();
        // Kept alive until `stop` has joined this thread: CFRunLoopStop on a run loop whose thread
        // already exited (the tap was invalidated) is a pointer-authentication trap that aborts the app.
        CFRetain(rl);
        CFRunLoopAddSource(rl, src, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        let _ = tx.send(Ok(Started {
            runloop: rl as usize,
            active: is_active,
        }));
        CFRunLoopRun();
        CGEventTapEnable(tap, false);
        CFMachPortInvalidate(tap);
        CFRelease(src);
        CFRelease(tap);
        drop(Box::from_raw(ctx));
    }
}

pub fn start(engine: Arc<Engine>, want_swallow: bool) -> Result<(HookHandle, HookInfo), HookError> {
    if !unsafe { CGPreflightListenEventAccess() } {
        return Err(HookError::Permission(PermKind::InputMonitoring));
    }
    let active = want_swallow && unsafe { AXIsProcessTrusted() };
    let (tx, rx) = mpsc::channel();
    let join = std::thread::Builder::new()
        .name("lowk-input-tap".into())
        .spawn(move || tap_thread(engine, active, tx))
        .map_err(|e| HookError::Failed(e.to_string()))?;
    let started = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(HookError::Failed(e)),
        Err(_) => return Err(HookError::Failed("event tap thread did not start".into())),
    };
    let rl = started.runloop;
    let stop = Box::new(move || {
        // CFRunLoopStop is a no-op until the loop is running: retry until the thread exits.
        for _ in 0..200 {
            if !join.is_finished() {
                unsafe { CFRunLoopStop(rl as CFTypeRef) };
            }
            if join.is_finished() {
                let _ = join.join();
                // SAFETY: balances the CFRetain taken on the tap thread.
                unsafe { CFRelease(rl as CFTypeRef) };
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    Ok((
        HookHandle::new(stop),
        HookInfo {
            swallow_capable: started.active,
        },
    ))
}

pub fn permissions() -> PermSnapshot {
    PermSnapshot {
        input_monitoring: Some(unsafe { CGPreflightListenEventAccess() }),
        accessibility: Some(unsafe { AXIsProcessTrusted() }),
    }
}

pub fn request_input_monitoring() -> bool {
    unsafe { CGRequestListenEventAccess() }
}

pub fn request_accessibility() -> bool {
    // AXIsProcessTrustedWithOptions(prompt) needs CFDictionary plumbing; the app shell
    // opens System Settings > Privacy > Accessibility instead. Report current state.
    unsafe { AXIsProcessTrusted() }
}

pub fn cursor_position() -> (f64, f64) {
    // SAFETY: a null-source event carries the current mouse location (top-left origin).
    unsafe {
        let e = CGEventCreate(null_mut());
        if e.is_null() {
            return (0.0, 0.0);
        }
        let p = CGEventGetLocation(e);
        CFRelease(e);
        (p.x, p.y)
    }
}
