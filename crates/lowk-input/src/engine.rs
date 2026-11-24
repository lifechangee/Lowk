//! Shared core between a native hook thread and the control side: owns the
//! matcher, stamps events, and implements record mode. Hook callbacks call
//! [`Engine::on_raw`] only; it never blocks on anything but a short mutex.
use crate::matcher::{Fire, Matcher, Raw};
use lowk_types::{now_mono_ns, BindEvent, BindSpec, KeyCode, Mods, MouseBtn, Trigger};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

struct Recorder {
    tx: Sender<Trigger>,
    mods: Mods,
    peak: Mods,
    key_used: bool,
    /// Releases of captured inputs to swallow once.
    swallow_up_keys: Vec<&'static str>,
    swallow_up_mouse: Vec<MouseBtn>,
}

fn weight(m: &Mods) -> u8 {
    m.ctrl as u8 + m.alt as u8 + m.shift as u8 + m.meta as u8
}

#[derive(Default)]
struct Inner {
    matcher: Matcher,
    recorder: Option<Recorder>,
    pending_up: (Vec<&'static str>, Vec<MouseBtn>),
}

pub struct Engine {
    core: Arc<Core>,
    timer: Mutex<Option<std::thread::JoinHandle<()>>>,
}

struct Core {
    inner: Mutex<Inner>,
    wake: Condvar,
    tx: Sender<BindEvent>,
    shutdown: std::sync::atomic::AtomicBool,
}

impl Engine {
    pub fn new(tx: Sender<BindEvent>) -> Self {
        Self {
            core: Arc::new(Core {
                inner: Mutex::new(Inner::default()),
                wake: Condvar::new(),
                tx,
                shutdown: std::sync::atomic::AtomicBool::new(false),
            }),
            timer: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.core.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn emit(&self, evs: impl IntoIterator<Item = Fire>) {
        for f in evs {
            tracing::debug!(action = ?f.action, phase = ?f.phase, "input: matched, queued for dispatch");
            let _ = self.core.tx.send(BindEvent {
                action: f.action,
                phase: f.phase,
                t_mono_ns: now_mono_ns(),
                template: f.template,
            });
        }
    }

    pub fn set_binds(&self, binds: Vec<BindSpec>) {
        let evs = self.lock().matcher.set_binds(binds);
        self.core.wake.notify_one();
        self.emit(evs);
    }

    pub fn set_hold_delay(&self, delay: Duration) {
        let mut guard = self.lock();
        let out = guard.matcher.set_hold_delay(delay);
        mask_alt_hold(&out);
        drop(guard);
        self.emit(out.events);
        self.core.wake.notify_one();
    }

    fn ensure_timer(&self) {
        if self.lock().matcher.next_deadline().is_none() {
            return;
        }
        let mut timer = self.timer.lock().unwrap_or_else(|e| e.into_inner());
        if timer.is_some() {
            self.core.wake.notify_one();
            return;
        }
        let weak = Arc::downgrade(&self.core);
        *timer = Some(
            std::thread::Builder::new()
                .name("lowk-input-hold-timer".into())
                .spawn(move || timer_loop(weak))
                .expect("spawn input hold timer"),
        );
    }

    /// Capture the next trigger. Binds are suspended while recording.
    pub fn start_record(&self) -> Receiver<Trigger> {
        let (tx, rx) = channel();
        let mut g = self.lock();
        let evs = g.matcher.release_all();
        g.recorder = Some(Recorder {
            tx,
            mods: Mods::NONE,
            peak: Mods::NONE,
            key_used: false,
            swallow_up_keys: vec![],
            swallow_up_mouse: vec![],
        });
        drop(g);
        self.core.wake.notify_one();
        self.emit(evs);
        rx
    }

    pub fn stop_record(&self) {
        self.lock().recorder = None;
    }

    pub fn is_recording(&self) -> bool {
        self.lock().recorder.is_some()
    }

    /// What the matcher believes is held: (non-modifier keys, mouse buttons). Health check input.
    pub fn held(&self) -> (Vec<&'static str>, u32) {
        let g = self.lock();
        (g.matcher.held_keys(), g.matcher.mouse_held())
    }

    /// Release keys whose key-up never reached the hook (each gets a normal `Up` through the matcher),
    /// and, when `mouse`, the mouse buttons too. A stuck key silences every modifier-only bind.
    pub fn release_stale(&self, keys: &[&'static str], mouse: bool) {
        for &code in keys {
            self.on_raw(Raw::Key {
                code,
                down: false,
                repeat: false,
            });
        }
        if mouse {
            let evs = self.lock().matcher.release_stale_mouse();
            self.emit(evs);
        }
    }

    /// See [`Matcher::set_baseline_mods`].
    pub fn baseline_mods(&self, m: Mods) {
        self.lock().matcher.set_baseline_mods(m);
        self.core.wake.notify_one();
    }

    /// Feed one native event. Returns true when the native event must be swallowed.
    pub fn on_raw(&self, raw: Raw) -> bool {
        let mut guard = self.lock();
        let g = &mut *guard;
        if let Some(rec) = g.recorder.as_mut() {
            let (swallow, done) = Self::record_step(rec, raw);
            // Keep matcher modifier state in sync; binds stay suspended (events dropped).
            match raw {
                Raw::Mods(m) => g.matcher.set_baseline_mods(m),
                Raw::Reset => {
                    g.matcher.handle(raw);
                }
                _ => {}
            }
            if let Some(t) = done {
                if let Some(rec) = g.recorder.take() {
                    let _ = rec.tx.send(t);
                    g.pending_up = (rec.swallow_up_keys, rec.swallow_up_mouse);
                }
            }
            drop(guard);
            self.core.wake.notify_one();
            return swallow;
        }
        if let Some(s) = take_pending(&mut g.pending_up, &raw) {
            return s;
        }
        let out = g.matcher.handle(raw);
        mask_alt_hold(&out);
        drop(guard);
        self.core.wake.notify_one();
        self.ensure_timer();
        if out.events.is_empty()
            && matches!(
                raw,
                Raw::Key {
                    down: true,
                    repeat: false,
                    ..
                }
            )
        {
            // Key content is never logged; this only proves the key reached the matcher.
            tracing::debug!("input: key down reached the matcher, no bind matched");
        }
        self.emit(out.events);
        out.swallow
    }

    fn record_step(rec: &mut Recorder, raw: Raw) -> (bool, Option<Trigger>) {
        match raw {
            Raw::Mods(m) => {
                rec.mods = m;
                if weight(&m) > weight(&rec.peak) {
                    rec.peak = m;
                }
                if m == Mods::NONE && rec.peak != Mods::NONE && !rec.key_used {
                    return (false, Some(Trigger::ModifierOnly { mods: rec.peak }));
                }
                (false, None)
            }
            Raw::Key {
                code,
                down: true,
                repeat,
            } => {
                if repeat || code == "Unknown" {
                    return (true, None);
                }
                rec.key_used = true;
                rec.swallow_up_keys.push(code);
                (
                    true,
                    Some(Trigger::Combo {
                        mods: rec.mods,
                        key: KeyCode::new(code),
                    }),
                )
            }
            Raw::Mouse { button, down: true } => {
                rec.key_used = true;
                rec.swallow_up_mouse.push(button);
                (
                    true,
                    Some(Trigger::Mouse {
                        button,
                        mods: rec.mods,
                    }),
                )
            }
            Raw::Key { .. } | Raw::Mouse { .. } | Raw::Reset => (false, None),
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        {
            let _guard = self.core.inner.lock().unwrap_or_else(|e| e.into_inner());
            self.core
                .shutdown
                .store(true, std::sync::atomic::Ordering::Release);
        }
        self.core.wake.notify_all();
        if let Some(timer) = self.timer.get_mut().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = timer.join();
        }
    }
}

fn timer_loop(weak: Weak<Core>) {
    loop {
        let Some(core) = weak.upgrade() else { return };
        if core.shutdown.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        let mut guard = core.inner.lock().unwrap_or_else(|e| e.into_inner());
        if core.shutdown.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        let Some(deadline) = guard.matcher.next_deadline() else {
            let g = core.wake.wait(guard).unwrap_or_else(|e| e.into_inner());
            drop(g);
            continue;
        };
        let now = Instant::now();
        if now < deadline {
            let (g, _) = core
                .wake
                .wait_timeout(guard, deadline - now)
                .unwrap_or_else(|e| e.into_inner());
            drop(g);
            continue;
        }
        let out = guard.matcher.poll(now);
        mask_alt_hold(&out);
        drop(guard);
        for f in out.events {
            tracing::debug!(action = ?f.action, phase = ?f.phase, "input: matched, queued for dispatch");
            let _ = core.tx.send(BindEvent {
                action: f.action,
                phase: f.phase,
                t_mono_ns: now_mono_ns(),
                template: f.template,
            });
        }
    }
}

fn mask_alt_hold(out: &crate::matcher::Outcome) {
    #[cfg(windows)]
    if out.mask_alt {
        crate::platform::win::mask_alt_hold();
    }
    #[cfg(not(windows))]
    let _ = out;
}

/// One-shot swallow of the releases belonging to a just-recorded key/button, so
/// the target app never sees an orphan key-up/mouse-up.
fn take_pending(p: &mut (Vec<&'static str>, Vec<MouseBtn>), raw: &Raw) -> Option<bool> {
    match *raw {
        Raw::Key {
            code, down: false, ..
        } => {
            let i = p.0.iter().position(|c| *c == code)?;
            p.0.remove(i);
            Some(true)
        }
        Raw::Mouse { button, down: false } => {
            let i = p.1.iter().position(|b| *b == button)?;
            p.1.remove(i);
            Some(true)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::{ActionId, Phase};

    fn engine() -> (Engine, Receiver<BindEvent>) {
        let (tx, rx) = channel();
        (Engine::new(tx), rx)
    }

    #[test]
    fn events_flow_and_stamp() {
        let (e, rx) = engine();
        e.set_binds(vec![BindSpec {
            action: ActionId::MoveHold,
            trigger: Trigger::ModifierOnly { mods: Mods::ALT },
            swallow: false,
            template: None,
        }]);
        e.on_raw(Raw::Mods(Mods::ALT));
        e.on_raw(Raw::Mods(Mods::NONE));
        let a = rx.try_recv().unwrap();
        let b = rx.try_recv().unwrap();
        assert_eq!((a.phase, b.phase), (Phase::Down, Phase::Up));
        assert!(b.t_mono_ns >= a.t_mono_ns);
    }

    #[test]
    fn timer_fires_without_more_input_and_drop_joins_worker() {
        let (e, rx) = engine();
        e.set_hold_delay(Duration::from_millis(50));
        e.set_binds(vec![BindSpec {
            action: ActionId::ClickHold,
            trigger: Trigger::ModifierOnly { mods: Mods::CTRL },
            swallow: false,
            template: None,
        }]);
        e.on_raw(Raw::Mods(Mods::CTRL));
        let down = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("timer should fire pending hold");
        assert_eq!((down.action, down.phase), (ActionId::ClickHold, Phase::Down));
        let started = Instant::now();
        drop(e);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "timer thread did not stop promptly"
        );
    }

    #[test]
    fn record_combo_swallows_and_is_one_shot() {
        let (e, rx) = engine();
        let rec = e.start_record();
        assert!(!e.on_raw(Raw::Mods(Mods::META_SHIFT)));
        assert!(e.on_raw(Raw::Key {
            code: "KeyJ",
            down: true,
            repeat: false
        }));
        assert_eq!(
            rec.try_recv().unwrap(),
            Trigger::Combo {
                mods: Mods::META_SHIFT,
                key: KeyCode::new("KeyJ")
            }
        );
        assert!(!e.is_recording());
        // release of the recorded key is swallowed once, then normal
        let up = Raw::Key {
            code: "KeyJ",
            down: false,
            repeat: false,
        };
        assert!(e.on_raw(up));
        assert!(!e.on_raw(up));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn record_modifier_only_on_release() {
        let (e, _rx) = engine();
        let rec = e.start_record();
        e.on_raw(Raw::Mods(Mods::ALT));
        assert!(rec.try_recv().is_err());
        e.on_raw(Raw::Mods(Mods::NONE));
        assert_eq!(rec.try_recv().unwrap(), Trigger::ModifierOnly { mods: Mods::ALT });
    }

    #[test]
    fn record_mouse_button() {
        let (e, _rx) = engine();
        let rec = e.start_record();
        assert!(e.on_raw(Raw::Mouse {
            button: MouseBtn::Forward,
            down: true
        }));
        assert_eq!(
            rec.try_recv().unwrap(),
            Trigger::Mouse {
                button: MouseBtn::Forward,
                mods: Mods::NONE
            }
        );
    }

    #[test]
    fn binds_suspended_while_recording() {
        let (e, rx) = engine();
        e.set_binds(vec![BindSpec {
            action: ActionId::Ask,
            trigger: Trigger::Combo {
                mods: Mods::NONE,
                key: KeyCode::new("KeyJ"),
            },
            swallow: false,
            template: None,
        }]);
        let _rec = e.start_record();
        e.on_raw(Raw::Key {
            code: "KeyJ",
            down: true,
            repeat: false,
        });
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn releasing_a_key_whose_up_was_lost_revives_modifier_only_binds() {
        // The macOS regression: one lost key-up left a key «down» in the matcher for good, so Control /
        // Option alone never fired again while OS-registered combos kept working.
        let (e, rx) = engine();
        e.set_binds(vec![BindSpec {
            action: ActionId::ClickHold,
            trigger: Trigger::ModifierOnly { mods: Mods::CTRL },
            swallow: false,
            template: None,
        }]);
        e.on_raw(Raw::Key {
            code: "Tab",
            down: true,
            repeat: false,
        }); // its up never arrives
        e.on_raw(Raw::Mods(Mods::CTRL));
        e.on_raw(Raw::Mods(Mods::NONE));
        assert!(rx.try_recv().is_err(), "silenced by the phantom key");
        assert_eq!(e.held(), (vec!["Tab"], 0));
        e.release_stale(&["Tab"], false);
        assert_eq!(e.held(), (vec![], 0));
        e.on_raw(Raw::Mods(Mods::CTRL));
        e.on_raw(Raw::Mods(Mods::NONE));
        assert_eq!(rx.try_recv().unwrap().phase, Phase::Down);
        assert_eq!(rx.try_recv().unwrap().phase, Phase::Up);
    }

    #[test]
    fn releasing_a_lost_mouse_up_ends_the_hold_it_started() {
        let (e, rx) = engine();
        e.set_binds(vec![BindSpec {
            action: ActionId::Ask,
            trigger: Trigger::Mouse {
                button: MouseBtn::Forward,
                mods: Mods::NONE,
            },
            swallow: false,
            template: None,
        }]);
        e.on_raw(Raw::Mouse {
            button: MouseBtn::Forward,
            down: true,
        });
        assert_eq!(rx.try_recv().unwrap().phase, Phase::Down);
        assert_eq!(e.held().1, 1);
        e.release_stale(&[], true);
        assert_eq!(e.held().1, 0);
        assert_eq!(rx.try_recv().unwrap().phase, Phase::Up);
    }
}
