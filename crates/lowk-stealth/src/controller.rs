//! `OverlayCtl`: the `OverlayController` implementation (mode machine +
//! magnet + guard), and `PillCtl`.

use crate::backend::WindowBackend;
use crate::guard::{run_main_sync, StealthGuard};
use crate::magnet::Magnet;
use crate::mode::{Interaction, ModeInput, ModeMachine};
use lowk_types::*;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const OVERLAY: &str = "overlay";
pub const MAIN: &str = "main";
pub const PILL: &str = "pill";
pub const AREA: &str = "area";

/// Window events the app forwards (Tauri `WindowEvent` equivalents).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEventKind {
    Focused(bool),
    Moved,
    Resized,
    ScaleFactorChanged,
    ThemeChanged,
    DisplayChanged,
    SpaceChanged,
    Wake,
}

struct Follow {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
}

pub struct OverlayCtl {
    pub(crate) guard: StealthGuard,
    main: Arc<dyn MainThread>,
    backend: Arc<dyn WindowBackend>,
    main_backend: Mutex<Option<Arc<dyn WindowBackend>>>,
    machine: Arc<Mutex<ModeMachine>>,
    applied: Arc<Mutex<Interaction>>,
    sink: Mutex<Option<Arc<dyn EventSink>>>,
    follow: Mutex<Option<Follow>>,
    last_reassert: Mutex<Instant>,
    pub(crate) resample: AtomicBool,
    capture: Mutex<Option<Arc<dyn ScreenGrab>>>,
}

impl OverlayCtl {
    pub(crate) fn new(
        guard: StealthGuard,
        main: Arc<dyn MainThread>,
        backend: Arc<dyn WindowBackend>,
    ) -> Arc<Self> {
        Arc::new(Self {
            guard,
            main,
            backend,
            main_backend: Mutex::new(None),
            machine: Arc::new(Mutex::new(ModeMachine::new())),
            applied: Arc::new(Mutex::new(Interaction::Passive)),
            sink: Mutex::new(None),
            follow: Mutex::new(None),
            last_reassert: Mutex::new(Instant::now()),
            resample: AtomicBool::new(false),
            capture: Mutex::new(None),
        })
    }

    pub(crate) fn set_sink(&self, s: Arc<dyn EventSink>) {
        *self.sink.lock() = Some(s);
    }
    pub(crate) fn set_main_backend(&self, b: Option<Arc<dyn WindowBackend>>) {
        *self.main_backend.lock() = b;
    }
    /// Pixel layer of the self-test: a grab *without* window exclusion. The
    /// overlay UI must paint a magenta sentinel (`r>170, b>170, g<100`) meanwhile.
    pub fn set_selftest_capture(&self, c: Arc<dyn ScreenGrab>) {
        *self.capture.lock() = Some(c);
    }

    pub fn frame(&self) -> Rect {
        let b = self.backend.clone();
        run_main_sync(&self.main, move || b.frame())
    }

    /// Move the overlay (global top-left). Used to restore remembered positions.
    pub fn set_origin(&self, x: f64, y: f64) {
        let b = self.backend.clone();
        run_main_sync(&self.main, move || b.set_origin(x, y));
        self.resample.store(true, Ordering::SeqCst);
    }

    /// Visible frames of all displays (global top-left).
    pub fn display_frames(&self) -> Vec<Rect> {
        let b = self.backend.clone();
        run_main_sync(&self.main, move || b.display_frames())
    }

    fn emit_mode(&self, m: OverlayMode) {
        if let Some(s) = self.sink.lock().clone() {
            s.emit(AppEvent::Overlay(OverlayEvent::Mode(m)));
        }
    }

    /// Bring the native style in line with the machine (runs on the main
    /// thread, callable from any). The machine lock is never held across native calls.
    fn sync_style(&self) -> Result<()> {
        let (b, machine, applied) = (self.backend.clone(), self.machine.clone(), self.applied.clone());
        run_main_sync(&self.main, move || {
            let want = machine.lock().style();
            let mut cur = applied.lock();
            if want != *cur {
                if let Err(e) = b.set_interaction(want) {
                    machine.lock().force_style(*cur);
                    return Err(e);
                }
                *cur = want;
            }
            Ok(())
        })
    }

    fn input(&self, i: ModeInput) -> Result<()> {
        let change = self.machine.lock().apply(i);
        let Some(c) = change else { return Ok(()) };
        if c.style_changed {
            self.sync_style()?;
            // Re-verify (and repair) the capture exclusion after the style change. This
            // never hides or re-shows the window (F4): flags only.
            if self.guard.is_visible(OVERLAY) && self.guard.verify(OVERLAY) != StealthStatus::Verified {
                let _ = self.guard.apply(OVERLAY);
            }
        }
        if c.prev != c.now {
            self.emit_mode(c.now);
        }
        Ok(())
    }

    /// G6: hold-Ctrl down / up. Never hides or re-shows the window (flags only).
    pub fn hover(&self, phase: Phase) {
        // One line per hold: tells «the hold arrived but the overlay is hidden» from «the hold never arrived».
        tracing::info!(
            ?phase,
            overlay_visible = self.guard.is_visible(OVERLAY),
            "overlay hover hold"
        );
        let _ = self.input(match phase {
            Phase::Down => ModeInput::HoverDown,
            Phase::Up => ModeInput::HoverUp,
        });
    }

    /// Native click-through state (QA readback).
    pub fn ignores_mouse(&self) -> Option<bool> {
        let b = self.backend.clone();
        run_main_sync(&self.main, move || b.ignores_mouse())
    }

    /// Escape pressed in the interactive overlay.
    pub fn escape(&self) {
        let _ = self.input(ModeInput::Escape);
    }

    /// Forwarded window events. Re-applies are throttled (never at 120 Hz while
    /// moving; the release re-applies once).
    pub fn on_window_event(&self, ev: WindowEventKind) {
        if ev == WindowEventKind::Focused(false) && self.machine.lock().style() == Interaction::Interactive {
            let b = self.backend.clone();
            if !run_main_sync(&self.main, move || b.is_key()) {
                let _ = self.input(ModeInput::Blur);
            }
        }
        if matches!(
            ev,
            WindowEventKind::Moved
                | WindowEventKind::Resized
                | WindowEventKind::DisplayChanged
                | WindowEventKind::ScaleFactorChanged
        ) {
            self.resample.store(true, Ordering::SeqCst);
        }
        if self.machine.lock().is_moving() {
            return;
        }
        let mut last = self.last_reassert.lock();
        if last.elapsed() >= Duration::from_millis(100) {
            *last = Instant::now();
            drop(last);
            self.guard.on_policy_change();
        }
    }

    /// Magnet with an explicit cursor for the follow loop (tests / custom sources).
    pub fn move_hold_with(&self, phase: Phase, cursor: Arc<dyn CursorSource>) {
        match phase {
            Phase::Down => {
                if self.follow.lock().is_some() || !self.guard.is_visible(OVERLAY) {
                    return;
                }
                let frame = self.frame();
                let magnet = Magnet::begin((frame.x, frame.y), (frame.w, frame.h), cursor.position());
                let _ = self.input(ModeInput::MoveDown);
                let stop = Arc::new(AtomicBool::new(false));
                let (stop2, main, backend) = (stop.clone(), self.main.clone(), self.backend.clone());
                let latest = Arc::new(Mutex::new(cursor.position()));
                let pending = Arc::new(AtomicBool::new(false));
                let handle = std::thread::Builder::new()
                    .name("lowk-magnet".into())
                    .spawn(move || {
                        while !stop2.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(8)); // ~120 Hz
                            *latest.lock() = cursor.position();
                            if pending.swap(true, Ordering::SeqCst) {
                                continue; // coalesce: a move job is still queued
                            }
                            let (latest, pending, backend) =
                                (latest.clone(), pending.clone(), backend.clone());
                            main.run(Box::new(move || {
                                pending.store(false, Ordering::SeqCst);
                                let c = *latest.lock();
                                let (x, y) = magnet.target(c, &backend.display_frames());
                                backend.set_origin(x, y);
                            }));
                        }
                    })
                    .expect("spawn magnet");
                *self.follow.lock() = Some(Follow { stop, handle });
            }
            Phase::Up => {
                let f = self.follow.lock().take();
                if let Some(f) = f {
                    f.stop.store(true, Ordering::SeqCst);
                    let _ = f.handle.join();
                }
                let _ = self.input(ModeInput::MoveUp);
                // Re-apply stealth once after the move; resample the tone.
                let _ = self.guard.apply(OVERLAY);
                self.resample.store(true, Ordering::SeqCst);
            }
        }
    }

    fn count_color(rgba: &[u8], pred: impl Fn(u8, u8, u8) -> bool) -> usize {
        rgba.chunks_exact(4).filter(|p| pred(p[0], p[1], p[2])).count()
    }
}

impl OverlayController for OverlayCtl {
    fn show(&self) -> Result<()> {
        self.guard.show(OVERLAY)?;
        // show may reset style bits: re-sync the interaction style.
        *self.applied.lock() = Interaction::Passive;
        self.sync_style()
    }

    fn hide(&self) {
        if let Some(f) = self.follow.lock().take() {
            f.stop.store(true, Ordering::SeqCst);
            let _ = f.handle.join();
            let _ = self.input(ModeInput::MoveUp);
        }
        self.guard.hide(OVERLAY);
        let _ = self.input(ModeInput::Hidden);
    }

    fn panic_hide(&self) {
        self.guard.panic_hide();
        if self.guard.panic_active() {
            if let Some(f) = self.follow.lock().take() {
                f.stop.store(true, Ordering::SeqCst);
                let _ = f.handle.join();
                let _ = self.input(ModeInput::MoveUp);
            }
            let _ = self.input(ModeInput::Hidden);
        }
    }

    fn set_mode(&self, m: OverlayMode) -> Result<()> {
        if m == OverlayMode::Moving {
            return self.input(ModeInput::MoveDown);
        }
        if m == OverlayMode::Hover {
            return self.input(ModeInput::HoverDown);
        }
        self.input(ModeInput::Set(m))
    }

    fn mode(&self) -> OverlayMode {
        self.machine.lock().effective()
    }

    fn move_hold(&self, phase: Phase, cursor: Arc<dyn CursorSource>) {
        self.move_hold_with(phase, cursor);
    }

    fn status(&self) -> StealthStatus {
        self.guard.status()
    }

    fn selftest(&self) -> Result<SelfTestReport> {
        let mut details = String::new();
        let ov = self.guard.verify(OVERLAY);
        let mut overlay_hidden = ov == StealthStatus::Verified;
        details.push_str(&format!("overlay flag: {ov:?}; "));
        let mut main_visible = false;
        if let Some(mb) = self.main_backend.lock().clone() {
            main_visible = run_main_sync(&self.main, move || mb.is_visible());
            details.push_str(&format!("main window visible: {main_visible}; "));
        } else {
            details.push_str("main window not registered; ");
        }
        let cap = self.capture.lock().clone();
        if let Some(cap) = cap {
            if self.guard.is_visible(OVERLAY) {
                let f = self.frame();
                match cap.grab(f) {
                    Ok((px, ..)) => {
                        let n = Self::count_color(&px, |r, g, b| r > 170 && b > 170 && g < 100);
                        details.push_str(&format!("pixels: overlay sentinel px={n}; "));
                        if n > 500 {
                            overlay_hidden = false;
                        }
                    }
                    Err(e) => details.push_str(&format!("pixel layer unavailable ({e}); ")),
                }
            } else {
                details.push_str("pixel layer skipped (overlay hidden); ");
            }
        } else {
            details.push_str("pixel layer not configured; ");
        }
        Ok(SelfTestReport {
            overlay_hidden,
            main_visible,
            details,
        })
    }
}

/// Origin of the pill window docked to `edge` of `display` (centred along the edge).
pub fn pill_origin(edge: PillEdge, size: (f64, f64), display: Rect) -> (f64, f64) {
    match edge {
        PillEdge::Left => (display.x, display.y + (display.h - size.1) / 2.0),
        PillEdge::Right => (
            display.x + display.w - size.0,
            display.y + (display.h - size.1) / 2.0,
        ),
        PillEdge::Top => (display.x + (display.w - size.0) / 2.0, display.y),
        PillEdge::Bottom => (
            display.x + (display.w - size.0) / 2.0,
            display.y + display.h - size.1,
        ),
    }
}

/// The edge strip. Same stealth guarantees as the overlay (label `pill`).
pub struct PillCtl {
    guard: StealthGuard,
    main: Arc<dyn MainThread>,
    backend: Arc<dyn WindowBackend>,
    follow: Mutex<Option<Follow>>,
}

impl PillCtl {
    pub(crate) fn new(
        guard: StealthGuard,
        main: Arc<dyn MainThread>,
        backend: Arc<dyn WindowBackend>,
    ) -> Self {
        Self {
            guard,
            main,
            backend,
            follow: Mutex::new(None),
        }
    }
    /// Dock to `edge` of the primary display, then show (apply -> verify -> show).
    pub fn show(&self, edge: PillEdge) -> Result<()> {
        self.show_with(edge, |_| None)
    }
    /// Like [`PillCtl::show`], but a remembered origin for the display (`lookup` gets its
    /// visible frame; G2) wins over the docked position. The origin is clamped to the display.
    pub fn show_with(
        &self,
        edge: PillEdge,
        lookup: impl Fn(&Rect) -> Option<(f64, f64)> + Send + 'static,
    ) -> Result<()> {
        let b = self.backend.clone();
        run_main_sync(&self.main, move || {
            let f = b.frame();
            if let Some(d) = b.display_frames().first().copied() {
                let (x, y) = match lookup(&d) {
                    Some(p) => crate::magnet::clamp_origin(p, (f.w, f.h), (p.0, p.1), &[d]),
                    None => pill_origin(edge, (f.w, f.h), d),
                };
                b.set_origin(x, y);
            }
        });
        self.guard.show(PILL)
    }
    pub fn hide(&self) {
        self.stop_follow();
        self.guard.hide(PILL)
    }
    pub fn is_visible(&self) -> bool {
        self.guard.is_visible(PILL)
    }
    pub fn frame(&self) -> Rect {
        let b = self.backend.clone();
        run_main_sync(&self.main, move || b.frame())
    }
    pub fn display_frames(&self) -> Vec<Rect> {
        let b = self.backend.clone();
        run_main_sync(&self.main, move || b.display_frames())
    }
    /// Move to a global top-left origin, clamped to the display under the window's centre.
    pub fn set_origin(&self, x: f64, y: f64) {
        let b = self.backend.clone();
        run_main_sync(&self.main, move || {
            let f = b.frame();
            let c = (x + f.w / 2.0, y + f.h / 2.0);
            let (x, y) = crate::magnet::clamp_origin((x, y), (f.w, f.h), c, &b.display_frames());
            b.set_origin(x, y);
        });
    }
    fn stop_follow(&self) {
        if let Some(f) = self.follow.lock().take() {
            f.stop.store(true, Ordering::SeqCst);
            let _ = f.handle.join();
        }
    }
    /// G2: hold-to-move magnet for the pill (same mechanism as the overlay's).
    pub fn move_hold(&self, phase: Phase, cursor: Arc<dyn CursorSource>) {
        match phase {
            Phase::Down => {
                if self.follow.lock().is_some() || !self.guard.is_visible(PILL) {
                    return;
                }
                let f = self.frame();
                let magnet = Magnet::begin((f.x, f.y), (f.w, f.h), cursor.position());
                let stop = Arc::new(AtomicBool::new(false));
                let (stop2, main, backend) = (stop.clone(), self.main.clone(), self.backend.clone());
                let handle = std::thread::Builder::new()
                    .name("lowk-pill-magnet".into())
                    .spawn(move || {
                        while !stop2.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(8));
                            let c = cursor.position();
                            let backend = backend.clone();
                            main.run(Box::new(move || {
                                let (x, y) = magnet.target(c, &backend.display_frames());
                                backend.set_origin(x, y);
                            }));
                        }
                    })
                    .expect("spawn pill magnet");
                *self.follow.lock() = Some(Follow { stop, handle });
            }
            Phase::Up => {
                self.stop_follow();
                let _ = self.guard.apply(PILL);
            }
        }
    }
}

/// The area-selection window (label `area`): created hidden, stealth like the overlay (excluded from
/// sharing and from our own grabs), shown only while a selection is open. While open it takes the mouse
/// and, when the OS allows it without activating the app, the keyboard; closing gives focus back.
pub struct AreaCtl {
    guard: StealthGuard,
    main: Arc<dyn MainThread>,
    backend: Arc<dyn WindowBackend>,
}

impl AreaCtl {
    pub(crate) fn new(
        guard: StealthGuard,
        main: Arc<dyn MainThread>,
        backend: Arc<dyn WindowBackend>,
    ) -> Self {
        Self { guard, main, backend }
    }
    /// apply -> verify -> show, then mouse + keys. `Ok(true)` = keys (Esc / Enter) reach the window;
    /// `Ok(false)` = mouse only (right-click still cancels). Refused while panic hide is engaged.
    pub fn open(&self) -> Result<bool> {
        if self.guard.panic_active() {
            return Err(AppError::new(ErrorCode::StealthUnavailable).with_detail("panic hide is engaged"));
        }
        self.guard.show(AREA)?;
        let b = self.backend.clone();
        let r = run_main_sync(&self.main, move || {
            match b.set_interaction(Interaction::Interactive) {
                Ok(()) => Ok(true),
                // No key window: still take the mouse, without the keyboard and without activating the app.
                Err(_) => b.set_interaction(Interaction::Hover).map(|()| false),
            }
        });
        if r.is_err() {
            self.close();
        }
        r
    }
    /// Back to click-through (restores the previously focused window), then hide.
    pub fn close(&self) {
        let b = self.backend.clone();
        run_main_sync(&self.main, move || {
            let _ = b.set_interaction(Interaction::Passive);
        });
        self.guard.hide(AREA);
    }
    pub fn is_visible(&self) -> bool {
        self.guard.is_visible(AREA)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::policy::WindowPolicy;
    use lowk_types::fakes::{CollectEventSink, FixedCursor, InlineMainThread};

    fn setup() -> (Arc<OverlayCtl>, FakeBackend, Arc<CollectEventSink>) {
        let main: Arc<dyn MainThread> = Arc::new(InlineMainThread);
        let guard = StealthGuard::new(main.clone());
        let fb = FakeBackend::new();
        guard
            .protect_backend(OVERLAY, WindowPolicy::OVERLAY, Arc::new(fb.clone()))
            .unwrap();
        let ctl = OverlayCtl::new(guard, main, Arc::new(fb.clone()));
        let sink = Arc::new(CollectEventSink::default());
        ctl.set_sink(sink.clone());
        (ctl, fb, sink)
    }

    #[test]
    fn interactive_toggle_syncs_native_style_and_emits() {
        let (c, fb, sink) = setup();
        c.show().unwrap();
        c.set_mode(OverlayMode::Interactive).unwrap();
        assert_eq!(fb.0.lock().interaction, Some(Interaction::Interactive));
        assert_eq!(c.mode(), OverlayMode::Interactive);
        c.escape();
        assert_eq!(fb.0.lock().interaction, Some(Interaction::Passive));
        let evs = sink.0.lock().unwrap();
        let modes: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                AppEvent::Overlay(OverlayEvent::Mode(m)) => Some(*m),
                _ => None,
            })
            .collect();
        assert_eq!(modes, vec![OverlayMode::Interactive, OverlayMode::Passive]);
    }

    /// F4: mode flips (including Moving) never hide, re-show or re-order the window.
    #[test]
    fn mode_flips_never_hide_or_reshow() {
        let (c, fb, _) = setup();
        c.show().unwrap();
        let count = |fb: &FakeBackend, n: &str| fb.calls().iter().filter(|c| c.as_str() == n).count();
        let (hides, shows) = (count(&fb, "hide"), count(&fb, "show"));
        for _ in 0..20 {
            for m in [
                OverlayMode::Interactive,
                OverlayMode::Passive,
                OverlayMode::Moving,
            ] {
                c.set_mode(m).unwrap();
                assert!(fb.0.lock().visible, "{m:?}");
            }
            c.escape();
        }
        assert_eq!((count(&fb, "hide"), count(&fb, "show")), (hides, shows));
        assert!(fb.0.lock().excluded);
    }

    /// G6: the hold toggles click-through without hide/show and without ever becoming key.
    #[test]
    fn hover_hold_toggles_click_through_without_flicker_or_focus() {
        let (c, fb, sink) = setup();
        c.show().unwrap();
        let count = |fb: &FakeBackend, n: &str| fb.calls().iter().filter(|c| c.as_str() == n).count();
        let (hides, shows) = (count(&fb, "hide"), count(&fb, "show"));
        assert_eq!(c.ignores_mouse(), Some(true));
        for _ in 0..10 {
            c.hover(Phase::Down);
            assert_eq!(c.mode(), OverlayMode::Hover);
            assert_eq!(c.ignores_mouse(), Some(false));
            assert!(fb.0.lock().excluded && fb.0.lock().visible);
            c.hover(Phase::Up);
            assert_eq!(c.mode(), OverlayMode::Passive);
            assert_eq!(c.ignores_mouse(), Some(true));
        }
        assert_eq!((count(&fb, "hide"), count(&fb, "show")), (hides, shows));
        let n = sink
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, AppEvent::Overlay(OverlayEvent::Mode(OverlayMode::Hover))))
            .count();
        assert_eq!(n, 10);
    }

    #[test]
    fn stealth_flag_unchanged_by_mode() {
        let (c, fb, _) = setup();
        c.show().unwrap();
        for m in [
            OverlayMode::Interactive,
            OverlayMode::Moving,
            OverlayMode::Passive,
        ] {
            c.set_mode(m).unwrap();
            assert!(fb.0.lock().excluded, "{m:?}");
        }
    }

    #[test]
    fn blur_exits_interactive_only_when_not_key() {
        let (c, _fb, _) = setup();
        c.show().unwrap();
        c.set_mode(OverlayMode::Interactive).unwrap();
        c.on_window_event(WindowEventKind::Focused(false));
        assert_eq!(
            c.mode(),
            OverlayMode::Interactive,
            "fake backend still reports key"
        );
        c.escape();
        assert_eq!(c.mode(), OverlayMode::Passive);
    }

    struct Path(Mutex<Vec<(f64, f64)>>);
    impl CursorSource for Path {
        fn position(&self) -> (f64, f64) {
            let mut v = self.0.lock();
            if v.len() > 1 {
                v.remove(0)
            } else {
                v[0]
            }
        }
    }

    #[test]
    fn magnet_follows_then_restores_mode_and_stays_stealth() {
        let (c, fb, _) = setup();
        c.show().unwrap();
        c.set_mode(OverlayMode::Interactive).unwrap();
        fb.0.lock().origin = (100.0, 100.0);
        let path = Arc::new(Path(Mutex::new(vec![(150.0, 150.0), (400.0, 300.0)])));
        c.move_hold_with(Phase::Down, path);
        assert_eq!(c.mode(), OverlayMode::Moving);
        std::thread::sleep(Duration::from_millis(80));
        c.move_hold_with(Phase::Up, Arc::new(FixedCursor(0.0, 0.0)));
        assert_eq!(c.mode(), OverlayMode::Interactive);
        // grab offset (-50,-50) + cursor (400,300)
        assert_eq!(fb.0.lock().origin, (350.0, 250.0));
        assert!(fb.0.lock().excluded);
    }

    #[test]
    fn move_down_ignored_when_hidden() {
        let (c, _fb, _) = setup();
        c.move_hold_with(Phase::Down, Arc::new(FixedCursor(1.0, 1.0)));
        assert_eq!(c.mode(), OverlayMode::Passive);
    }

    #[test]
    fn hide_drops_interactive() {
        let (c, _fb, _) = setup();
        c.show().unwrap();
        c.set_mode(OverlayMode::Interactive).unwrap();
        c.hide();
        assert_eq!(c.mode(), OverlayMode::Passive);
    }

    #[test]
    fn selftest_reports_flag_state() {
        let (c, ..) = setup();
        c.show().unwrap();
        let r = c.selftest().unwrap();
        assert!(r.overlay_hidden);
        assert!(!r.main_visible);
    }

    fn pill_setup() -> (PillCtl, FakeBackend) {
        let main: Arc<dyn MainThread> = Arc::new(InlineMainThread);
        let guard = StealthGuard::new(main.clone());
        let fb = FakeBackend::new();
        guard
            .protect_backend(PILL, WindowPolicy::PILL, Arc::new(fb.clone()))
            .unwrap();
        {
            let mut s = fb.0.lock();
            s.size = (22.0, 88.0);
            s.displays = vec![Rect {
                x: 0.0,
                y: 0.0,
                w: 1000.0,
                h: 800.0,
            }];
        }
        (PillCtl::new(guard, main, Arc::new(fb.clone())), fb)
    }

    /// G2: the pill docks by default, honours a remembered origin (clamped), and the magnet moves it.
    #[test]
    fn pill_remembered_origin_and_magnet() {
        let (p, fb) = pill_setup();
        p.show(PillEdge::Right).unwrap();
        assert_eq!(fb.0.lock().origin, (978.0, 356.0));
        p.hide();
        p.show_with(PillEdge::Right, |_| Some((300.0, 200.0))).unwrap();
        assert_eq!(fb.0.lock().origin, (300.0, 200.0));
        p.hide();
        p.show_with(PillEdge::Right, |_| Some((5000.0, -50.0))).unwrap();
        assert_eq!(fb.0.lock().origin, (978.0, 0.0), "clamped into the display");
        let path = Arc::new(Path(Mutex::new(vec![(985.0, 10.0), (500.0, 400.0)])));
        p.move_hold(Phase::Down, path);
        std::thread::sleep(Duration::from_millis(80));
        p.move_hold(Phase::Up, Arc::new(FixedCursor(0.0, 0.0)));
        assert_eq!(fb.0.lock().origin, (493.0, 390.0));
        assert!(fb.0.lock().excluded && fb.0.lock().visible);
    }

    #[test]
    fn pill_origin_edges() {
        let d = Rect {
            x: 0.0,
            y: 0.0,
            w: 1000.0,
            h: 800.0,
        };
        assert_eq!(pill_origin(PillEdge::Right, (6.0, 80.0), d), (994.0, 360.0));
        assert_eq!(pill_origin(PillEdge::Left, (6.0, 80.0), d), (0.0, 360.0));
        assert_eq!(pill_origin(PillEdge::Top, (80.0, 6.0), d), (460.0, 0.0));
        assert_eq!(pill_origin(PillEdge::Bottom, (80.0, 6.0), d), (460.0, 794.0));
    }

    #[test]
    fn area_window_is_stealth_takes_input_while_open_and_gives_it_back() {
        let main: Arc<dyn MainThread> = Arc::new(InlineMainThread);
        let guard = StealthGuard::new(main.clone());
        let fb = FakeBackend::new();
        guard
            .protect_backend(AREA, WindowPolicy::AREA, Arc::new(fb.clone()))
            .unwrap();
        let a = AreaCtl::new(guard.clone(), main, Arc::new(fb.clone()));
        assert!(fb.0.lock().excluded, "flag set before the first show");
        assert!(a.open().unwrap());
        {
            let s = fb.0.lock();
            assert!(s.visible && s.excluded);
            assert_eq!(s.interaction, Some(Interaction::Interactive));
            let show = s.calls.iter().position(|c| c == "show").unwrap();
            let apply = s.calls.iter().position(|c| c == "apply").unwrap();
            assert!(apply < show, "apply before show");
        }
        a.close();
        let s = fb.0.lock();
        assert!(!s.visible);
        assert_eq!(s.interaction, Some(Interaction::Passive));
        drop(s);
        guard.panic_hide();
        assert!(a.open().is_err(), "no selection window under panic hide");
        assert!(!fb.0.lock().visible);
    }
}
