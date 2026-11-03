//! Pure recovery state machine for the macOS event tap (the OS calls are abstracted into [`TapOs`]
//! so it runs headless in tests).
//!
//! Why it exists: a CGEventTap can stop delivering without telling us. macOS sends
//! `kCGEventTapDisabledBy*` only for timeouts and user input; after sleep/wake, a session change or a
//! revoked-and-regranted permission the mach port is invalidated, the run loop has no source left and
//! `CFRunLoopRun` returns, so the tap thread used to exit silently while the binds kept reporting
//! «Active». Modifier-only binds (they only exist through the tap) then died for good while combos,
//! which go through the OS shortcut API, kept working.
//!
//! Only targeted recoveries are done here. There is deliberately no periodic «replace the tap while
//! idle» (the Windows workaround for hooks the OS drops silently): an idle snapshot does not exclude a
//! key arriving a moment later, and no macOS evidence says a healthy tap needs it.
use std::time::Duration;

/// Time the machine spent asleep between two ticks at least this long means a wake.
pub(crate) const WAKE_GAP: Duration = Duration::from_secs(5);
/// Longest wait between attempts to create the tap while that keeps failing.
pub(crate) const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Why the tap is being re-created. Logged at INFO with every recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    /// The run loop returned on its own (no source left: the port was invalidated).
    LoopEnded,
    /// `CFMachPortIsValid` is false.
    PortInvalid,
    /// The machine was asleep.
    Wake,
    /// Re-enabling did not take: the tap stays disabled.
    TapDisabled,
    /// The OS reports modifiers the tap never delivered for two ticks in a row.
    ModsDiverged,
    /// The previous attempt to create the tap failed.
    Retry,
}

impl Reason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Reason::LoopEnded => "run loop ended (tap port invalidated)",
            Reason::PortInvalid => "tap mach port invalid",
            Reason::Wake => "wake from sleep",
            Reason::TapDisabled => "tap stays disabled after re-enable",
            Reason::ModsDiverged => "modifier state diverged from the OS",
            Reason::Retry => "retry after a failed create",
        }
    }
}

/// One health sample. Everything the state machine needs, nothing OS specific.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Probe {
    /// Time spent asleep since the previous tick: `mach_continuous_time` (counts during sleep) minus
    /// `mach_absolute_time` (stops). Wall-clock changes and scheduling stalls do not show up here.
    pub slept: Duration,
    pub loop_ended: bool,
    pub port_valid: bool,
    pub tap_enabled: bool,
    /// Packed modifiers delivered by the tap vs reported by the OS.
    pub mods_tracked: u8,
    pub mods_actual: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Ok,
    /// Call `CGEventTapEnable(true)`; no state is lost.
    Reenable,
    Recreate(Reason),
}

#[derive(Default)]
pub(crate) struct TapHealth {
    reenabled: bool,
    mods_suspect: bool,
}

impl TapHealth {
    /// Decide the next step. Hard failures recreate at once (the tap is gone, nothing to lose); soft
    /// signals need to repeat on a second tick.
    pub(crate) fn step(&mut self, p: &Probe) -> Step {
        if p.loop_ended {
            return self.recreate(Reason::LoopEnded);
        }
        if !p.port_valid {
            return self.recreate(Reason::PortInvalid);
        }
        if p.slept >= WAKE_GAP {
            return self.recreate(Reason::Wake);
        }
        if !p.tap_enabled {
            if self.reenabled {
                return self.recreate(Reason::TapDisabled);
            }
            self.reenabled = true;
            return Step::Reenable;
        }
        self.reenabled = false;
        // A disagreement that lasts two ticks means events are not reaching us.
        if p.mods_tracked != p.mods_actual {
            if self.mods_suspect {
                return self.recreate(Reason::ModsDiverged);
            }
            self.mods_suspect = true;
        } else {
            self.mods_suspect = false;
        }
        Step::Ok
    }

    fn recreate(&mut self, r: Reason) -> Step {
        *self = Self::default();
        Step::Recreate(r)
    }
}

/// Delay before the next attempt after `failures` failed creations in a row (1, 2, 4 ... 30 s).
pub(crate) fn backoff(failures: u32) -> Duration {
    Duration::from_secs(1u64 << failures.min(5)).min(BACKOFF_MAX)
}

/// What the supervisor needs from the OS. The macOS backend implements it with CoreGraphics calls; the
/// tests with a fake.
pub(crate) trait TapOs {
    /// Input Monitoring is granted (creating a tap without it would raise the system prompt again).
    fn permitted(&self) -> bool;
    /// Create the tap, attach it to the run loop and set the modifier baseline from the OS (a modifier
    /// held across a recovery does not fire its bind; the next fresh press does). False when refused.
    fn create(&mut self) -> bool;
    /// Tear the tap down (safe when none exists).
    fn destroy(&mut self);
    fn enable(&mut self);
    /// Release everything the matcher believes is held (the tap lost events).
    fn reset_held(&mut self);
    /// Sample the OS: port, enabled flag, modifier state.
    fn sample(&mut self) -> OsSample;
    /// Release keys/buttons whose up was lost (two looks, or at once after a mode change). Only called
    /// while healthy.
    fn release_stale(&mut self, immediate: bool);
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OsSample {
    pub port_valid: bool,
    pub tap_enabled: bool,
    pub mods_tracked: u8,
    pub mods_actual: u8,
}

/// What a tick did; the supervisor already logged at INFO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tick {
    Quiet,
    Reenabled,
    Recreated(Reason),
    /// Creating failed; the next attempt is not before `retry_in` from now.
    CreateFailed {
        retry_in: Duration,
    },
}

pub(crate) struct Supervisor<O: TapOs> {
    pub os: O,
    health: TapHealth,
    up: bool,
    failures: u32,
    retry_at: Duration,
}

impl<O: TapOs> Supervisor<O> {
    /// The tap has just been created successfully.
    pub(crate) fn started(os: O) -> Self {
        Self {
            os,
            health: TapHealth::default(),
            up: true,
            failures: 0,
            retry_at: Duration::ZERO,
        }
    }

    pub(crate) fn is_up(&self) -> bool {
        self.up
    }

    /// One health check. `now`: monotonic time of the supervisor; `slept`: time asleep since the last
    /// tick; `loop_ended`: the run loop returned because it had no source left.
    pub(crate) fn tick(&mut self, now: Duration, slept: Duration, loop_ended: bool) -> Tick {
        if !self.up {
            if now < self.retry_at {
                return Tick::Quiet;
            }
            return self.rebuild(Reason::Retry, now);
        }
        let s = self.os.sample();
        let p = Probe {
            slept,
            loop_ended,
            port_valid: s.port_valid,
            tap_enabled: s.tap_enabled,
            mods_tracked: s.mods_tracked,
            mods_actual: s.mods_actual,
        };
        match self.health.step(&p) {
            Step::Ok => {
                self.os.release_stale(false);
                Tick::Quiet
            }
            Step::Reenable => {
                tracing::info!("input: event tap was disabled, re-enabling it");
                self.os.enable();
                Tick::Reenabled
            }
            Step::Recreate(r) => self.rebuild(r, now),
        }
    }

    fn rebuild(&mut self, reason: Reason, now: Duration) -> Tick {
        self.os.reset_held();
        self.os.destroy();
        self.up = false;
        if !self.os.permitted() {
            self.failures += 1;
            let wait = backoff(self.failures);
            self.retry_at = now + wait;
            tracing::warn!(
                reason = reason.as_str(),
                "input: event tap not re-created, Input Monitoring is not granted"
            );
            return Tick::CreateFailed { retry_in: wait };
        }
        if self.os.create() {
            self.up = true;
            self.failures = 0;
            self.health = TapHealth::default();
            tracing::info!(reason = reason.as_str(), "input: event tap re-created");
            Tick::Recreated(reason)
        } else {
            self.failures += 1;
            let wait = backoff(self.failures);
            self.retry_at = now + wait;
            tracing::warn!(
                reason = reason.as_str(),
                retry_in_s = wait.as_secs(),
                "input: event tap could not be created, will retry"
            );
            Tick::CreateFailed { retry_in: wait }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::matcher::Raw;
    use lowk_types::{ActionId, BindSpec, Mods, Phase, Trigger};
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::{Arc, Mutex};
    const S: fn(u64) -> Duration = Duration::from_secs;

    fn healthy() -> Probe {
        Probe {
            slept: Duration::ZERO,
            loop_ended: false,
            port_valid: true,
            tap_enabled: true,
            mods_tracked: 0,
            mods_actual: 0,
        }
    }

    #[test]
    fn a_healthy_tap_is_left_alone() {
        let mut h = TapHealth::default();
        for _ in 0..10 {
            assert_eq!(h.step(&healthy()), Step::Ok);
        }
    }

    #[test]
    fn a_run_loop_that_ended_on_its_own_is_recreated() {
        // The reported bug: the tap thread used to exit here and never come back.
        let mut h = TapHealth::default();
        let p = Probe {
            loop_ended: true,
            ..healthy()
        };
        assert_eq!(h.step(&p), Step::Recreate(Reason::LoopEnded));
    }

    #[test]
    fn an_invalidated_port_is_recreated() {
        let mut h = TapHealth::default();
        let p = Probe {
            port_valid: false,
            ..healthy()
        };
        assert_eq!(h.step(&p), Step::Recreate(Reason::PortInvalid));
    }

    #[test]
    fn time_asleep_recreates_but_a_short_stall_does_not() {
        let mut h = TapHealth::default();
        assert_eq!(
            h.step(&Probe {
                slept: S(4),
                ..healthy()
            }),
            Step::Ok
        );
        assert_eq!(
            h.step(&Probe {
                slept: S(3600),
                ..healthy()
            }),
            Step::Recreate(Reason::Wake)
        );
    }

    #[test]
    fn a_disabled_tap_is_re_enabled_once_then_recreated() {
        let mut h = TapHealth::default();
        let off = Probe {
            tap_enabled: false,
            ..healthy()
        };
        assert_eq!(h.step(&off), Step::Reenable);
        assert_eq!(h.step(&off), Step::Recreate(Reason::TapDisabled));
        assert_eq!(h.step(&off), Step::Reenable);
        assert_eq!(h.step(&healthy()), Step::Ok);
        assert_eq!(
            h.step(&off),
            Step::Reenable,
            "a one-off disable does not accumulate"
        );
    }

    #[test]
    fn diverging_modifiers_need_two_ticks() {
        let mut h = TapHealth::default();
        let d = Probe {
            mods_actual: 0b10,
            ..healthy()
        };
        assert_eq!(h.step(&d), Step::Ok, "first look only suspects");
        assert_eq!(h.step(&healthy()), Step::Ok, "agreement clears the suspicion");
        assert_eq!(h.step(&d), Step::Ok);
        assert_eq!(h.step(&d), Step::Recreate(Reason::ModsDiverged));
    }

    #[test]
    fn hard_failures_beat_everything_else() {
        let mut h = TapHealth::default();
        let p = Probe {
            loop_ended: true,
            port_valid: false,
            tap_enabled: false,
            slept: S(100),
            ..healthy()
        };
        assert_eq!(h.step(&p), Step::Recreate(Reason::LoopEnded));
    }

    #[test]
    fn creation_retries_back_off_up_to_thirty_seconds() {
        let v: Vec<u64> = (0..8).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(v, [1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[derive(Default)]
    struct FakeOs {
        permitted: bool,
        create_ok: bool,
        sample: Option<OsSample>,
        calls: Vec<&'static str>,
    }
    fn ok_sample() -> OsSample {
        OsSample {
            port_valid: true,
            tap_enabled: true,
            mods_tracked: 0,
            mods_actual: 0,
        }
    }
    impl TapOs for FakeOs {
        fn permitted(&self) -> bool {
            self.permitted
        }
        fn create(&mut self) -> bool {
            self.calls.push("create");
            self.create_ok
        }
        fn destroy(&mut self) {
            self.calls.push("destroy");
        }
        fn enable(&mut self) {
            self.calls.push("enable");
        }
        fn reset_held(&mut self) {
            self.calls.push("reset");
        }
        fn sample(&mut self) -> OsSample {
            self.sample.unwrap_or_else(ok_sample)
        }
        fn release_stale(&mut self, immediate: bool) {
            self.calls.push(if immediate { "stale!" } else { "stale" });
        }
    }
    fn sup() -> Supervisor<FakeOs> {
        Supervisor::started(FakeOs {
            permitted: true,
            create_ok: true,
            ..Default::default()
        })
    }

    #[test]
    fn supervisor_recovers_after_sleep_and_releases_held_state_first() {
        let mut s = sup();
        assert_eq!(s.tick(S(1), S(0), false), Tick::Quiet);
        assert_eq!(s.tick(S(7200), S(7199), false), Tick::Recreated(Reason::Wake));
        let c = &s.os.calls;
        let (r, d, n) = (
            c.iter().position(|x| *x == "reset").unwrap(),
            c.iter().position(|x| *x == "destroy").unwrap(),
            c.iter().position(|x| *x == "create").unwrap(),
        );
        assert!(r < d && d < n, "reset, destroy, then create: {c:?}");
        assert!(s.is_up());
        assert_eq!(s.tick(S(7201), S(0), false), Tick::Quiet);
    }

    #[test]
    fn supervisor_keeps_retrying_with_backoff_until_the_tap_can_be_created() {
        let mut s = sup();
        s.os.create_ok = false;
        s.os.sample = Some(OsSample {
            port_valid: false,
            ..ok_sample()
        });
        assert_eq!(s.tick(S(10), S(0), false), Tick::CreateFailed { retry_in: S(2) });
        assert!(!s.is_up());
        assert_eq!(
            s.tick(S(11), S(0), false),
            Tick::Quiet,
            "waiting for the back-off"
        );
        assert_eq!(s.tick(S(12), S(0), false), Tick::CreateFailed { retry_in: S(4) });
        s.os.create_ok = true;
        assert_eq!(s.tick(S(16), S(0), false), Tick::Recreated(Reason::Retry));
        s.os.sample = None;
        assert_eq!(s.tick(S(17), S(0), false), Tick::Quiet);
    }

    #[test]
    fn supervisor_never_creates_a_tap_without_the_permission() {
        let mut s = sup();
        s.os.permitted = false;
        assert!(matches!(s.tick(S(1), S(0), true), Tick::CreateFailed { .. }));
        assert!(!s.os.calls.contains(&"create"), "{:?}", s.os.calls);
        s.os.permitted = true;
        assert_eq!(s.tick(S(10), S(0), false), Tick::Recreated(Reason::Retry));
    }

    #[test]
    fn supervisor_re_enables_without_losing_state() {
        let mut s = sup();
        s.os.sample = Some(OsSample {
            tap_enabled: false,
            ..ok_sample()
        });
        assert_eq!(s.tick(S(1), S(0), false), Tick::Reenabled);
        assert_eq!(s.os.calls, vec!["enable"]);
    }

    #[test]
    fn supervisor_checks_stale_keys_only_while_healthy() {
        let mut s = sup();
        s.tick(S(1), S(0), false);
        assert_eq!(s.os.calls, vec!["stale"]);
        s.os.calls.clear();
        s.tick(S(2), S(0), true);
        assert!(!s.os.calls.contains(&"stale"));
    }

    #[test]
    fn a_healthy_tap_is_never_replaced_however_long_it_idles() {
        let mut s = sup();
        for t in 1..=600 {
            assert_eq!(s.tick(S(t), S(0), false), Tick::Quiet);
        }
        assert!(!s.os.calls.contains(&"create") && !s.os.calls.contains(&"destroy"));
    }

    /// A fake OS over a real engine: `os_mods` is what the hardware says, `tracked` what the callback saw.
    struct EngineOs {
        engine: Arc<Engine>,
        os_mods: Arc<Mutex<Mods>>,
        tracked: Mods,
    }
    fn pack(m: Mods) -> u8 {
        m.shift as u8 | (m.ctrl as u8) << 1 | (m.alt as u8) << 2 | (m.meta as u8) << 3
    }
    impl TapOs for EngineOs {
        fn permitted(&self) -> bool {
            true
        }
        fn create(&mut self) -> bool {
            let m = *self.os_mods.lock().unwrap();
            self.tracked = m;
            self.engine.baseline_mods(m);
            true
        }
        fn destroy(&mut self) {}
        fn enable(&mut self) {}
        fn reset_held(&mut self) {
            self.tracked = Mods::NONE;
            self.engine.on_raw(Raw::Reset);
        }
        fn sample(&mut self) -> OsSample {
            OsSample {
                port_valid: true,
                tap_enabled: true,
                mods_tracked: pack(self.tracked),
                mods_actual: pack(*self.os_mods.lock().unwrap()),
            }
        }
        fn release_stale(&mut self, _immediate: bool) {}
    }

    #[test]
    fn recovery_while_control_is_held_rebuilds_once_fires_nothing_and_the_next_press_fires() {
        let (tx, rx): (_, Receiver<_>) = channel();
        let engine = Arc::new(Engine::new(tx));
        engine.set_binds(vec![BindSpec {
            action: ActionId::ClickHold,
            trigger: Trigger::ModifierOnly { mods: Mods::CTRL },
            swallow: false,
            template: None,
        }]);
        let os_mods = Arc::new(Mutex::new(Mods::NONE));
        let mut s = Supervisor::started(EngineOs {
            engine: engine.clone(),
            os_mods: os_mods.clone(),
            tracked: Mods::NONE,
        });
        // Control goes down and the hold starts.
        *os_mods.lock().unwrap() = Mods::CTRL;
        s.os.tracked = Mods::CTRL;
        engine.on_raw(Raw::Mods(Mods::CTRL));
        assert_eq!(rx.try_recv().unwrap().phase, Phase::Down);
        // The machine wakes with Control still held: one recovery, the hold ends.
        assert_eq!(s.tick(S(1), S(3600), false), Tick::Recreated(Reason::Wake));
        assert_eq!(rx.try_recv().unwrap().phase, Phase::Up);
        for t in 2..12 {
            assert_eq!(s.tick(S(t), S(0), false), Tick::Quiet, "no repeated rebuilds");
        }
        assert!(rx.try_recv().is_err(), "the held modifier does not fire again");
        // Release (the callback sees it), then a fresh press fires exactly once.
        *os_mods.lock().unwrap() = Mods::NONE;
        s.os.tracked = Mods::NONE;
        engine.on_raw(Raw::Mods(Mods::NONE));
        assert!(rx.try_recv().is_err());
        *os_mods.lock().unwrap() = Mods::CTRL;
        s.os.tracked = Mods::CTRL;
        engine.on_raw(Raw::Mods(Mods::CTRL));
        assert_eq!(rx.try_recv().unwrap().phase, Phase::Down);
        assert!(rx.try_recv().is_err());
    }
}
