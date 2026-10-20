//! Stealth: WindowPolicy, StealthGuard, ModeMachine, magnet, panic hide,
//! adaptive tone, self-test, per-OS native code.
//!
//! Rules (docs/ARCHITECTURE.md 7): Rust owns every stealth call; every native
//! window call runs on the main thread (injected [`MainThread`]); fail closed
//! (apply -> verify -> show); focus (mode machine) is orthogonal to stealth.
//! This crate takes `raw-window-handle` handles, never `tauri::Window`.

pub mod backend;
pub mod controller;
pub mod guard;
pub mod magnet;
pub mod mode;
mod native;
pub mod policy;
pub mod tone;

pub use backend::{Readback, WindowBackend};
pub use controller::{OverlayCtl, PillCtl, WindowEventKind, MAIN, OVERLAY, PILL};
pub use guard::StealthGuard;
pub use mode::{Interaction, ModeChange, ModeInput, ModeMachine};
pub use policy::{WindowPolicy, WindowRole};
pub use tone::AdaptiveTone;

use lowk_types::*;
use parking_lot::Mutex;
use raw_window_handle::RawWindowHandle;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A native window handle that may cross threads. The handle is only
/// dereferenced on the main thread, by the backend.
#[derive(Clone, Copy)]
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub struct RawWindow(RawWindowHandle);

// SAFETY: the handle is an opaque pointer/integer; the crate only uses it on
// the main thread, where the owning toolkit guarantees the window is alive
// (windows are permanent: the overlay is never destroyed).
unsafe impl Send for RawWindow {}
unsafe impl Sync for RawWindow {}

impl RawWindow {
    pub fn new(h: RawWindowHandle) -> Self {
        Self(h)
    }
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub(crate) fn handle(&self) -> RawWindowHandle {
        self.0
    }
}

/// Native handles of the labelled windows (`overlay` required).
#[derive(Default)]
pub struct RawWindows {
    pub overlay: Option<RawWindow>,
    pub main: Option<RawWindow>,
    pub pill: Option<RawWindow>,
}

#[derive(Clone, Copy)]
pub struct StealthConfig {
    pub hide_main: bool,
    /// Watchdog period while a stealth window is visible (default 1000 ms).
    pub verify_interval_ms: u32,
}

impl Default for StealthConfig {
    fn default() -> Self {
        Self {
            hide_main: false,
            verify_interval_ms: 1000,
        }
    }
}

pub struct Stealth {
    pub controller: Arc<dyn OverlayController>,
    /// Concrete controller: events, escape, explicit-cursor magnet, geometry.
    pub overlay: Arc<OverlayCtl>,
    pub guard: StealthGuard,
    pub pill: Option<PillCtl>,
    tone: Arc<ToneShared>,
    _watchdog: std::thread::JoinHandle<()>,
}

struct ToneShared {
    state: Mutex<AdaptiveTone>,
    cb: Mutex<Box<dyn Fn(Tone) + Send>>,
}

/// Stops the tone sampler thread on drop.
pub struct ToneSampler {
    stop: Arc<AtomicBool>,
}
impl Drop for ToneSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub fn create_stealth(
    win: RawWindows,
    main_thread: Arc<dyn MainThread>,
    cfg: StealthConfig,
    tone: Box<dyn Fn(Tone) + Send>,
) -> Result<Stealth> {
    let overlay_raw = win
        .overlay
        .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument).with_detail("overlay window missing"))?;
    let guard = StealthGuard::new(main_thread.clone());

    let ob = native::make_backend(&overlay_raw)?;
    let mut main_be = None;
    let mut pill = None;
    // Register the main window first: its failure must never affect the overlay.
    if let Some(m) = win.main {
        let b = native::make_backend(&m)?;
        guard.protect_backend(MAIN, WindowPolicy::main(cfg.hide_main), b.clone())?;
        main_be = Some(b);
    }
    guard.protect_backend(OVERLAY, WindowPolicy::OVERLAY, ob.clone())?;
    if let Some(p) = win.pill {
        let b = native::make_backend(&p)?;
        guard.protect_backend(PILL, WindowPolicy::PILL, b.clone())?;
        pill = Some(PillCtl::new(guard.clone(), main_thread.clone(), b));
    }

    let ctl = OverlayCtl::new(guard.clone(), main_thread.clone(), ob);
    if let Some(b) = main_be {
        ctl.set_main_backend(Some(b));
    }
    let g2 = guard.clone();
    native::install_observers(Arc::new(move || g2.on_policy_change()));
    let watchdog = guard.spawn_watchdog(Duration::from_millis(u64::from(cfg.verify_interval_ms.max(100))));
    Ok(Stealth {
        controller: ctl.clone(),
        overlay: ctl,
        guard,
        pill,
        tone: Arc::new(ToneShared {
            state: Mutex::new(AdaptiveTone::new(ToneMode::Auto)),
            cb: Mutex::new(tone),
        }),
        _watchdog: watchdog,
    })
}

impl Stealth {
    /// Where `AppEvent::Overlay(Mode)` and `AppEvent::Stealth` go.
    pub fn set_event_sink(&self, sink: Arc<dyn EventSink>) {
        self.guard.set_sink(sink.clone());
        self.overlay.set_sink(sink);
    }

    /// Register the main window (created on demand, may be re-created). It is
    /// never stealth unless `hide_main`; a failure never touches the overlay.
    pub fn register_main(&self, win: RawWindow, hide_main: bool) -> Result<()> {
        let b = native::make_backend(&win)?;
        self.guard
            .protect_backend(MAIN, WindowPolicy::main(hide_main), b.clone())?;
        self.overlay.set_main_backend(Some(b));
        Ok(())
    }

    /// Drop the main window before it is destroyed.
    pub fn unregister_main(&self) {
        self.overlay.set_main_backend(None);
        self.guard.unprotect(MAIN);
    }

    /// "Hide main from capture" setting; affects only the `main` label.
    pub fn set_hide_main(&self, on: bool) -> Result<()> {
        self.guard.set_policy(MAIN, WindowPolicy::main(on))
    }

    /// Last emitted (or forced) tone, for a window that loads after the event.
    pub fn current_tone(&self) -> Tone {
        self.tone.state.lock().current()
    }

    /// Forced palette (`auto|light|dark`); emits the new tone immediately.
    pub fn set_tone_mode(&self, mode: ToneMode) {
        let mut s = self.tone.state.lock();
        s.set_mode(mode);
        if mode != ToneMode::Auto {
            let t = s.current();
            drop(s);
            (self.tone.cb.lock())(t);
        }
    }

    /// Forward a window event of `label` (Tauri `WindowEvent`).
    pub fn on_window_event(&self, label: &str, ev: WindowEventKind) {
        if label == OVERLAY {
            self.overlay.on_window_event(ev);
        } else {
            self.guard.on_policy_change();
        }
    }

    /// Start the sampler task: every ~2 s while the overlay is visible, plus a
    /// debounced 150 ms re-sample after move/resize/display change. Unchanged
    /// bitmaps are skipped. Needs the sampler to exclude our own windows.
    pub fn spawn_tone_sampler(&self, sampler: Arc<dyn RegionSampler>) -> ToneSampler {
        let stop = Arc::new(AtomicBool::new(false));
        let (stop2, ctl, tone) = (stop.clone(), self.overlay.clone(), self.tone.clone());
        std::thread::Builder::new()
            .name("lowk-tone".into())
            .spawn(move || {
                let mut last_sample = Instant::now() - Duration::from_secs(10);
                let mut flagged: Option<Instant> = None;
                let mut last_hash = 0u64;
                while !stop2.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(50));
                    if ctl.resample.swap(false, Ordering::SeqCst) {
                        flagged = Some(Instant::now());
                    }
                    let due = last_sample.elapsed() >= Duration::from_secs(2)
                        || flagged.is_some_and(|t| t.elapsed() >= Duration::from_millis(150));
                    if !due || !ctl.guard.is_visible(OVERLAY) {
                        continue;
                    }
                    flagged = None;
                    last_sample = Instant::now();
                    let frame = ctl.frame();
                    let Ok(bmp) = sampler.grab_region(frame, tone::SAMPLE_W, tone::SAMPLE_H) else {
                        continue;
                    };
                    let mut h = DefaultHasher::new();
                    bmp.hash(&mut h);
                    if h.finish() == last_hash {
                        continue;
                    }
                    last_hash = h.finish();
                    let t = tone.state.lock().update(&bmp, tone::SAMPLE_W, tone::SAMPLE_H);
                    if let Some(t) = t {
                        (tone.cb.lock())(t);
                    }
                }
            })
            .expect("spawn tone sampler");
        ToneSampler { stop }
    }
}
