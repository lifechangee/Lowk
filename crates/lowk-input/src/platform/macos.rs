//! macOS: CGEventTap on a dedicated CFRunLoop thread (raw FFI, no extra crates).
//! Input Monitoring is required for the tap; swallowing needs an active tap
//! (Accessibility). Without Accessibility we fall back to a listen-only tap.
use super::{confirm_stale, take_resync, HookError, HookHandle, HookInfo, LiveInfo, PermSnapshot};
use crate::engine::Engine;
use crate::health::{OsSample, Supervisor, TapOs, WAKE_GAP};
use crate::keys::from_mac_vk;
use crate::matcher::Raw;
use lowk_types::{Mods, MouseBtn, PermKind};
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

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
    fn CGEventTapIsEnabled(tap: CFTypeRef) -> bool;
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    fn CGEventSourceFlagsState(state: i32) -> u64;
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
    static kCFRunLoopDefaultMode: CFTypeRef;
    fn CFMachPortIsValid(port: CFTypeRef) -> bool;
    fn CFRunLoopRunInMode(mode: CFTypeRef, seconds: f64, return_after_source: bool) -> i32;
    fn CFRunLoopRemoveSource(rl: CFTypeRef, source: CFTypeRef, mode: CFTypeRef);
    fn CFMachPortCreateRunLoopSource(alloc: CFTypeRef, port: CFTypeRef, order: isize) -> CFTypeRef;
    fn CFMachPortInvalidate(port: CFTypeRef);
    fn CFRunLoopGetCurrent() -> CFTypeRef;
    fn CFRunLoopAddSource(rl: CFTypeRef, source: CFTypeRef, mode: CFTypeRef);
    fn CFRunLoopStop(rl: CFTypeRef);
    fn CFRelease(cf: CFTypeRef);
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
}

#[repr(C)]
struct TimebaseInfo {
    numer: u32,
    denom: u32,
}

extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_continuous_time() -> u64;
    fn mach_timebase_info(info: *mut TimebaseInfo) -> i32;
}

/// `(continuous, absolute)` mach clocks in nanoseconds. The continuous clock keeps counting while the
/// machine sleeps, the absolute one stops, so their difference is the time spent asleep. Wall-clock
/// changes and scheduling stalls move both equally.
fn mach_clocks_ns() -> (u64, u64) {
    let mut tb = TimebaseInfo { numer: 0, denom: 0 };
    // SAFETY: plain libSystem calls with a valid out pointer.
    let (c, a) = unsafe {
        mach_timebase_info(&mut tb);
        (mach_continuous_time(), mach_absolute_time())
    };
    let f = |t: u64| (t as u128 * tb.numer.max(1) as u128 / tb.denom.max(1) as u128) as u64;
    (f(c), f(a))
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

const RL_FINISHED: i32 = 1;
const HID_STATE: i32 = 1;

/// The tap and its run-loop source; both die together.
struct Tap {
    port: CFTypeRef,
    src: CFTypeRef,
}

/// Everything the supervisor needs from CoreGraphics, over the leaked `Ctx` of this tap thread.
struct MacOs {
    ctx: *mut Ctx,
    want_swallow: bool,
    rl: CFTypeRef,
    tap: Option<Tap>,
    active: bool,
    suspect_keys: Vec<&'static str>,
    suspect_mouse: bool,
    live: Arc<LiveInfo>,
}

impl MacOs {
    fn engine(&self) -> &Engine {
        // SAFETY: `ctx` lives until the tap thread ends, which is after this struct is dropped.
        unsafe { &(*self.ctx).engine }
    }

    fn port(&self) -> CFTypeRef {
        self.tap.as_ref().map_or(null_mut(), |t| t.port)
    }
}

fn os_key_down(code: &'static str) -> bool {
    match crate::keys::to_mac_vk(code) {
        Some(vk) => unsafe { CGEventSourceKeyState(HID_STATE, vk) },
        // A key we have no name for: any key down counts as «it may still be held».
        None => (0u16..128).any(|vk| unsafe { CGEventSourceKeyState(HID_STATE, vk) }),
    }
}

fn os_mouse_down() -> bool {
    (2u32..16).any(|b| unsafe { CGEventSourceButtonState(HID_STATE, b) })
}

impl TapOs for MacOs {
    fn permitted(&self) -> bool {
        unsafe { CGPreflightListenEventAccess() }
    }

    fn create(&mut self) -> bool {
        let mask: u64 =
            (1 << KEY_DOWN) | (1 << KEY_UP) | (1 << FLAGS_CHANGED) | (1 << OTHER_DOWN) | (1 << OTHER_UP);
        let ctx = self.ctx;
        let make = |active: bool| unsafe {
            // HID tap (0) sees events earliest; head insert (0).
            CGEventTapCreate(
                0,
                0,
                if active { 0 } else { 1 },
                mask,
                callback,
                ctx as *mut c_void,
            )
        };
        let mut active = self.want_swallow && unsafe { AXIsProcessTrusted() };
        let mut port = make(active);
        if port.is_null() && active {
            active = false;
            port = make(false);
        }
        if port.is_null() {
            return false;
        }
        unsafe {
            let src = CFMachPortCreateRunLoopSource(null_mut(), port, 0);
            if src.is_null() {
                CFMachPortInvalidate(port);
                CFRelease(port);
                return false;
            }
            (*ctx).tap.store(port, Ordering::Release);
            // Baseline from the OS for both the callback and the matcher: a modifier held across a
            // recovery does not fire its bind (the next fresh press does), and it is not a «divergence».
            let held = mods_from_flags(CGEventSourceFlagsState(HID_STATE));
            (*ctx).last_mods.store(pack(held), Ordering::Relaxed);
            (*ctx).engine.baseline_mods(held);
            CFRunLoopAddSource(self.rl, src, kCFRunLoopCommonModes);
            CGEventTapEnable(port, true);
            self.tap = Some(Tap { port, src });
        }
        if self.want_swallow && !active && self.live.current().is_none_or(|i| i.swallow_capable) {
            tracing::warn!("input: event tap is listen-only now (Accessibility is not granted): swallowing binds leak their keys");
        }
        self.live.publish(true, active);
        self.active = active;
        self.suspect_keys.clear();
        self.suspect_mouse = false;
        true
    }

    fn destroy(&mut self) {
        self.live.publish(false, false);
        let Some(t) = self.tap.take() else { return };
        unsafe {
            (*self.ctx).tap.store(null_mut(), Ordering::Release);
            CGEventTapEnable(t.port, false);
            CFRunLoopRemoveSource(self.rl, t.src, kCFRunLoopCommonModes);
            CFMachPortInvalidate(t.port);
            CFRelease(t.src);
            CFRelease(t.port);
        }
    }

    fn enable(&mut self) {
        let p = self.port();
        if !p.is_null() {
            unsafe { CGEventTapEnable(p, true) };
        }
    }

    fn reset_held(&mut self) {
        unsafe { (*self.ctx).last_mods.store(0, Ordering::Relaxed) };
        self.engine().on_raw(Raw::Reset);
    }

    fn sample(&mut self) -> OsSample {
        let port = self.port();
        let tracked = unsafe { (*self.ctx).last_mods.load(Ordering::Relaxed) };
        let actual = pack(mods_from_flags(unsafe { CGEventSourceFlagsState(HID_STATE) }));
        OsSample {
            port_valid: !port.is_null() && unsafe { CFMachPortIsValid(port) },
            tap_enabled: !port.is_null() && unsafe { CGEventTapIsEnabled(port) },
            mods_tracked: tracked,
            mods_actual: actual,
        }
    }

    fn release_stale(&mut self, immediate: bool) {
        let resync = take_resync();
        let immediate = immediate || resync;
        let (keys, mouse) = self.engine().held();
        let stale = confirm_stale(&mut self.suspect_keys, &keys, os_key_down, immediate);
        let mouse_stale = if mouse > 0 && !os_mouse_down() {
            let was = std::mem::replace(&mut self.suspect_mouse, true);
            was || immediate
        } else {
            self.suspect_mouse = false;
            false
        };
        if mouse_stale {
            self.suspect_mouse = false;
        }
        if !stale.is_empty() || mouse_stale {
            // Counts only: key content is never logged.
            tracing::info!(
                keys = stale.len(),
                mouse = mouse_stale,
                "input: released keys/buttons whose up event was lost"
            );
            self.engine().release_stale(&stale, mouse_stale);
        }
    }
}

fn tap_thread(
    engine: Arc<Engine>,
    want_swallow: bool,
    stop: Arc<AtomicBool>,
    live: Arc<LiveInfo>,
    tx: mpsc::Sender<Result<Started, String>>,
) {
    let ctx = Box::into_raw(Box::new(Ctx {
        engine,
        tap: AtomicPtr::new(null_mut()),
        last_mods: AtomicU8::new(0),
    }));
    // Two references: this thread's own (released when it ends) and one handed to the stop closure,
    // which releases it after joining. `stop` never touches a run loop it does not own a reference to.
    let rl = unsafe { CFRetain(CFRunLoopGetCurrent()) };
    unsafe { CFRetain(rl) };
    let mut os = MacOs {
        ctx,
        want_swallow,
        rl,
        tap: None,
        active: false,
        suspect_keys: vec![],
        suspect_mouse: false,
        live,
    };
    if !os.create() {
        let _ = tx.send(Err("CGEventTapCreate returned null".into()));
        drop(unsafe { Box::from_raw(ctx) });
        unsafe {
            CFRelease(rl);
            CFRelease(rl);
        }
        return;
    }
    let _ = tx.send(Ok(Started {
        runloop: rl as usize,
        active: os.active,
    }));
    // The run loop is driven in one-second slices instead of `CFRunLoopRun`: each slice is a health
    // check (see `health.rs`). `CFRunLoopRun` used to return silently when the tap's port was invalidated
    // (sleep/wake, session change), ending this thread while the binds still read «Active».
    let epoch = Instant::now();
    let mut sup = Supervisor::started(os);
    let mut last_clocks = mach_clocks_ns();
    while !stop.load(Ordering::Acquire) {
        let res = if sup.is_up() {
            unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 1.0, false) }
        } else {
            // Nothing to run: wait out the back-off in short slices so `stop` stays prompt.
            std::thread::sleep(Duration::from_millis(250));
            0
        };
        if stop.load(Ordering::Acquire) {
            break;
        }
        let clocks = mach_clocks_ns();
        let slept = Duration::from_nanos(
            (clocks.0.saturating_sub(last_clocks.0)).saturating_sub(clocks.1.saturating_sub(last_clocks.1)),
        );
        last_clocks = clocks;
        let now = epoch.elapsed();
        // A panic in the health check must not end the thread: treat it as a lost tap.
        let r = catch_unwind(AssertUnwindSafe(|| sup.tick(now, slept, res == RL_FINISHED)));
        if r.is_err() {
            tracing::error!("input: event tap health check panicked, re-creating the tap");
            sup.os.destroy();
            let _ = sup.tick(now, WAKE_GAP, true);
        }
    }
    sup.os.destroy();
    unsafe {
        CFRelease(rl);
        drop(Box::from_raw(ctx));
    }
}

pub fn start(engine: Arc<Engine>, want_swallow: bool) -> Result<(HookHandle, HookInfo), HookError> {
    if !unsafe { CGPreflightListenEventAccess() } {
        return Err(HookError::Permission(PermKind::InputMonitoring));
    }
    let (tx, rx) = mpsc::channel();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let thread_stop = stop_flag.clone();
    let live = Arc::new(LiveInfo::new(false, false));
    let thread_live = live.clone();
    let join = std::thread::Builder::new()
        .name("lowk-input-tap".into())
        .spawn(move || tap_thread(engine, want_swallow, thread_stop, thread_live, tx))
        .map_err(|e| HookError::Failed(e.to_string()))?;
    let started = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(HookError::Failed(e)),
        Err(_) => return Err(HookError::Failed("event tap thread did not start".into())),
    };
    let rl = started.runloop;
    // `rl` carries a reference owned by this closure (taken on the tap thread), so it is valid memory
    // for the whole shutdown whatever the thread does. The flag ends the loop within a second on its own;
    // CFRunLoopStop only makes it prompt.
    let stop = Box::new(move || {
        stop_flag.store(true, Ordering::Release);
        for _ in 0..400 {
            if join.is_finished() {
                break;
            }
            unsafe { CFRunLoopStop(rl as CFTypeRef) };
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = join.join();
        // SAFETY: balances the reference taken for this closure on the tap thread.
        unsafe { CFRelease(rl as CFTypeRef) };
    });
    Ok((
        HookHandle::new(stop).with_live(live),
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

/// The tap thread polls the refresh request every second (see `tap_thread`); nothing to wake.
pub fn wake() {}
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
