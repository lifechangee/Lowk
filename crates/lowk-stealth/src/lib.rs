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
pub mod palette;
pub mod policy;
#[cfg(any(test, feature = "swatches"))]
pub mod swatches;
pub mod tone;

pub use backend::{Readback, WindowBackend};
pub use controller::{AreaCtl, OverlayCtl, PillCtl, WindowEventKind, AREA, MAIN, OVERLAY, PILL};
pub use guard::StealthGuard;
pub use mode::{Interaction, ModeChange, ModeInput, ModeMachine};
pub use policy::{WindowPolicy, WindowRole};
pub use tone::AdaptiveTone;

use lowk_types::*;
use parking_lot::Mutex;
use raw_window_handle::RawWindowHandle;
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
    pub area: Option<RawWindow>,
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
    /// The area-selection window (macOS and Windows; absent when its window was not supplied).
    pub area: Option<AreaCtl>,
    tone: Arc<ToneShared>,
    _watchdog: std::thread::JoinHandle<()>,
}

struct ToneShared {
    state: Mutex<AdaptiveTone>,
    cb: Mutex<Box<dyn Fn(Tone) + Send>>,
    /// Region (global top-left points) of the last backdrop grab: QA reads it to prove the sampler looks where the overlay is.
    last_rect: Mutex<Option<Rect>>,
    /// Sampler counters and the last raw result, so a silently failing sampler is visible (QA `tone-diag`).
    diag: Mutex<SamplerDiag>,
}

#[derive(Default)]
struct SamplerDiag {
    ticks: u64,
    grab_errors: u64,
    last_error: Option<String>,
    admitted: u64,
    emitted: u64,
    /// Successful grabs; QA waits for it to advance past a layout ack so it never compares a stale frame.
    frames: u64,
    /// Raw mean colour of each grid cell of the last bitmap the sampler got.
    cell_means: Vec<[u8; 3]>,
    /// `"stream"` (continuous capture), `"poll"` (fallback) or `"idle"` (not started / overlay hidden).
    mode: &'static str,
    /// Frames the OS delivered / that changed and were analysed, per second over the last >= 1 s window
    /// (poll mode: successful grabs / admitted bitmaps). CPU cost scales with `processed_per_s`.
    frames_per_s: f64,
    processed_per_s: f64,
    /// Totals behind the rates (stream: `delivered`, `processed`).
    delivered: u64,
    processed: u64,
    rate_at: Option<Instant>,
    rate_base: (u64, u64),
}

impl SamplerDiag {
    /// Refresh the per-second rates from the running totals (called every loop turn, updates once per second).
    fn update_rates(&mut self, now: Instant) {
        match self.rate_at {
            None => {
                self.rate_at = Some(now);
                self.rate_base = (self.delivered, self.processed);
            }
            Some(t) if now.duration_since(t) >= Duration::from_secs(1) => {
                let dt = now.duration_since(t).as_secs_f64();
                self.frames_per_s = (self.delivered.saturating_sub(self.rate_base.0)) as f64 / dt;
                self.processed_per_s = (self.processed.saturating_sub(self.rate_base.1)) as f64 / dt;
                self.rate_at = Some(now);
                self.rate_base = (self.delivered, self.processed);
            }
            _ => {}
        }
    }
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

/// Owns the running tone sampler (or nothing). The adaptive-scrim setting drives it: while the setting is off nothing
/// is constructed and no screen is ever grabbed; turning it on starts the sampler, turning it off drops (stops) it.
/// Generic over the handle so tests can use a stand-in for [`ToneSampler`].
pub struct SamplerSlot<T = ToneSampler> {
    slot: Mutex<Option<T>>,
}
impl<T> Default for SamplerSlot<T> {
    fn default() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }
}
impl<T> SamplerSlot<T> {
    /// Brings the slot in line with `on`. `start` runs only when the sampler has to be started (never while off,
    /// never twice in a row). Returns whether a sampler is running afterwards.
    pub fn set_enabled(&self, on: bool, start: impl FnOnce() -> T) -> bool {
        let mut g = self.slot.lock();
        match (on, g.is_some()) {
            (true, false) => *g = Some(start()),
            (false, true) => *g = None, // Drop stops the sampler thread
            _ => {}
        }
        g.is_some()
    }
    pub fn is_running(&self) -> bool {
        self.slot.lock().is_some()
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
    let mut area = None;
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
    if let Some(a) = win.area {
        let b = native::make_backend(&a)?;
        guard.protect_backend(AREA, WindowPolicy::AREA, b.clone())?;
        area = Some(AreaCtl::new(guard.clone(), main_thread.clone(), b));
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
        area,
        tone: Arc::new(ToneShared {
            state: Mutex::new(AdaptiveTone::new(ToneMode::Auto)),
            cb: Mutex::new(tone),
            last_rect: Mutex::new(None),
            diag: Mutex::new(SamplerDiag::default()),
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

    /// Region of the last backdrop grab (QA).
    pub fn last_sample_rect(&self) -> Option<Rect> {
        *self.tone.last_rect.lock()
    }

    /// JSON: sampler counters, last grab error, raw per-cell mean colours of the last bitmap (QA).
    pub fn sampler_diag_json(&self) -> String {
        let d = self.tone.diag.lock();
        let cells: Vec<String> = d
            .cell_means
            .iter()
            .map(|c| format!("[{},{},{}]", c[0], c[1], c[2]))
            .collect();
        format!(
            "{{\"ticks\":{},\"grab_errors\":{},\"last_error\":{},\"admitted\":{},\"emitted\":{},\"frames\":{},\"mode\":\"{}\",\"frames_per_s\":{:.1},\"processed_per_s\":{:.1},\"delivered\":{},\"processed\":{},\"cell_means\":[{}]}}",
            d.ticks,
            d.grab_errors,
            d.last_error.as_ref().map_or("null".to_string(), |e| format!("{e:?}")),
            d.admitted,
            d.emitted,
            d.frames,
            if d.mode.is_empty() { "idle" } else { d.mode },
            d.frames_per_s,
            d.processed_per_s,
            d.delivered,
            d.processed,
            cells.join(",")
        )
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

    /// Start the sampler task. Preferred path: a continuous capture stream of the region under the overlay
    /// ([`RegionSampler::open_stream`]); it exists only while the overlay is visible (hidden or sampler dropped =
    /// stream dropped = fully stopped), follows the overlay when it moves or resizes, and delivers a frame only when
    /// the pixels changed. Fallback (no stream support, start failure, stream error): every `tone::POLL_MS` (100 ms)
    /// and at once after a move/resize/display change, unchanged bitmaps skipped. The sampler excludes our own windows.
    pub fn spawn_tone_sampler(&self, sampler: Arc<dyn RegionSampler>) -> ToneSampler {
        let stop = Arc::new(AtomicBool::new(false));
        let (stop2, ctl, tone) = (stop.clone(), self.overlay.clone(), self.tone.clone());
        std::thread::Builder::new()
            .name("lowk-tone".into())
            .spawn(move || {
                let mut gate = tone::ChangeGate::default();
                let mut last_poll = Instant::now() - Duration::from_secs(10);
                let mut flagged = false;
                let mut stream: Option<Box<dyn lowk_types::RegionStream>> = None;
                let mut stream_rect: Option<Rect> = None;
                let mut last_bmp: Option<Vec<u8>> = None;
                let mut last_frame_at = Instant::now();
                let mut retry_stream_at = Instant::now();
                let analyse = |bmp: &[u8]| {
                    {
                        let mut d = tone.diag.lock();
                        d.cell_means = tone::cell_means(bmp, tone::SAMPLE_W, tone::SAMPLE_H);
                    }
                    let t = tone.state.lock().update(bmp, tone::SAMPLE_W, tone::SAMPLE_H);
                    if let Some(t) = t {
                        tone.diag.lock().emitted += 1;
                        (tone.cb.lock())(t);
                    }
                };
                while !stop2.load(Ordering::SeqCst) {
                    let visible = ctl.guard.is_visible(OVERLAY);
                    if !visible {
                        // Hidden: nothing may run (the stream is stopped, not paused).
                        if stream.take().is_some() {
                            stream_rect = None;
                        }
                        {
                            let mut d = tone.diag.lock();
                            d.mode = "idle";
                            d.update_rates(Instant::now());
                        }
                        std::thread::sleep(Duration::from_millis(25));
                        flagged |= ctl.resample.swap(false, Ordering::SeqCst);
                        continue;
                    }
                    // -- stream path --
                    if stream.is_none() && Instant::now() >= retry_stream_at {
                        let frame = ctl.frame();
                        stream = sampler.open_stream(frame, tone::SAMPLE_W, tone::SAMPLE_H);
                        if stream.is_some() {
                            stream_rect = Some(frame);
                            last_frame_at = Instant::now();
                        } else {
                            retry_stream_at = Instant::now() + Duration::from_secs(10);
                        }
                    }
                    if let Some(st) = stream.as_mut() {
                        let frame = ctl.frame();
                        flagged |= ctl.resample.swap(false, Ordering::SeqCst);
                        let mut broken = None;
                        if stream_rect != Some(frame) {
                            if let Err(e) = st.retarget(frame) {
                                broken = Some(format!("{e:?}"));
                            }
                            stream_rect = Some(frame);
                        }
                        *tone.last_rect.lock() = Some(frame);
                        if broken.is_none() {
                            broken = st.error();
                        }
                        if let Some(e) = broken {
                            let mut d = tone.diag.lock();
                            d.grab_errors += 1;
                            d.last_error = Some(e);
                            drop(d);
                            stream = None; // stop it; poll for a while, then try the stream again
                            retry_stream_at = Instant::now() + Duration::from_secs(10);
                            continue;
                        }
                        let got = st.next_frame(Duration::from_millis(25));
                        let (delivered, processed) = st.counters();
                        {
                            let mut d = tone.diag.lock();
                            d.mode = "stream";
                            d.ticks += 1;
                            d.delivered = delivered;
                            d.processed = processed;
                            d.frames = processed;
                            d.admitted = processed;
                            d.update_rates(Instant::now());
                        }
                        if let Some(bmp) = got {
                            last_frame_at = Instant::now();
                            analyse(&bmp);
                            last_bmp = Some(bmp);
                        } else if last_frame_at.elapsed() >= Duration::from_millis(tone::SETTLE_MS)
                            && tone.state.lock().has_pending()
                        {
                            // A stream sends nothing while the screen is still: confirm a pending change by re-feeding
                            // the last bitmap (a blip would have been cancelled by a newer frame by now).
                            last_frame_at = Instant::now();
                            if let Some(b) = last_bmp.clone() {
                                analyse(&b);
                            }
                        }
                        continue;
                    }
                    // -- polled fallback --
                    std::thread::sleep(Duration::from_millis(25));
                    // a move / resize / display change polls right away (debounced by the 25 ms tick)
                    flagged |= ctl.resample.swap(false, Ordering::SeqCst);
                    if !flagged && last_poll.elapsed() < Duration::from_millis(tone::POLL_MS) {
                        continue;
                    }
                    flagged = false;
                    last_poll = Instant::now();
                    let frame = ctl.frame();
                    *tone.last_rect.lock() = Some(frame);
                    {
                        let mut d = tone.diag.lock();
                        d.mode = "poll";
                        d.ticks += 1;
                    }
                    let bmp = match sampler.grab_region(frame, tone::SAMPLE_W, tone::SAMPLE_H) {
                        Ok(b) => b,
                        Err(e) => {
                            let mut d = tone.diag.lock();
                            d.grab_errors += 1;
                            d.last_error = Some(format!("{e:?}"));
                            continue;
                        }
                    };
                    {
                        let mut d = tone.diag.lock();
                        d.frames += 1;
                        d.delivered = d.frames;
                    }
                    let admitted = gate.admit(&bmp);
                    {
                        let mut d = tone.diag.lock();
                        if admitted {
                            d.admitted += 1;
                        }
                        d.processed = d.admitted;
                        d.update_rates(Instant::now());
                    }
                    if admitted {
                        analyse(&bmp);
                    }
                }
            })
            .expect("spawn tone sampler");
        ToneSampler { stop }
    }
}

#[cfg(test)]
mod sampler_slot_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Stand-in for the real capture: counts every grab.
    #[derive(Default)]
    struct CountingSampler(AtomicUsize);
    impl RegionSampler for CountingSampler {
        fn grab_region(&self, _r: Rect, _w: u32, _h: u32) -> Result<Vec<u8>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(vec![])
        }
    }
    /// Stand-in for `ToneSampler`: "stops" when dropped.
    struct Handle(Arc<AtomicBool>);
    impl Drop for Handle {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn sampler_is_not_started_while_the_adaptive_scrim_is_off() {
        let (starts, stopped) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
        let sampler = Arc::new(CountingSampler::default());
        let slot: SamplerSlot<Handle> = SamplerSlot::default();
        let start = || {
            starts.fetch_add(1, Ordering::SeqCst);
            Handle(stopped.clone())
        };
        let off = OverlaySettings::default();
        assert!(!off.adaptive_scrim, "default: off");
        // startup with the default settings and repeated "off" applications: nothing is created, nothing grabbed
        for _ in 0..3 {
            assert!(!slot.set_enabled(off.sampler_wanted(), start));
        }
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        assert_eq!(sampler.0.load(Ordering::SeqCst), 0, "no screen grab while off");
        assert!(!slot.is_running());

        // turned on at runtime: starts once (idempotent)
        let on = OverlaySettings {
            adaptive_scrim: true,
            ..OverlaySettings::default()
        };
        assert!(slot.set_enabled(on.sampler_wanted(), start));
        assert!(slot.set_enabled(on.sampler_wanted(), start));
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert!(!stopped.load(Ordering::SeqCst));

        // turned off again: the sampler is stopped
        assert!(!slot.set_enabled(off.sampler_wanted(), start));
        assert!(
            stopped.load(Ordering::SeqCst),
            "the sampler thread is told to stop"
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn plain_scrim_is_a_fixed_preset_until_adaptive_is_on() {
        let mut o = OverlaySettings::default();
        assert_eq!(
            o.effective_tone_mode(),
            ToneMode::Dark,
            "auto without sampling = the plain dark scrim"
        );
        o.tone_mode = ToneMode::Light;
        assert_eq!(
            o.effective_tone_mode(),
            ToneMode::Light,
            "a forced light palette still applies"
        );
        o.tone_mode = ToneMode::Auto;
        o.adaptive_scrim = true;
        assert_eq!(o.effective_tone_mode(), ToneMode::Auto);
    }
}
