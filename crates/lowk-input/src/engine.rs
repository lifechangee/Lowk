//! Shared core between a native hook thread and the control side: owns the
//! matcher, stamps events, and implements record mode. Hook callbacks call
//! [`Engine::on_raw`] only; it never blocks on anything but a short mutex.
use crate::matcher::{Fire, Matcher, Raw};
use lowk_types::{now_mono_ns, BindEvent, BindSpec, KeyCode, Mods, MouseBtn, Trigger};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

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
    inner: Mutex<Inner>,
    tx: Sender<BindEvent>,
}

impl Engine {
    pub fn new(tx: Sender<BindEvent>) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            tx,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn emit(&self, evs: impl IntoIterator<Item = Fire>) {
        for f in evs {
            tracing::debug!(action = ?f.action, phase = ?f.phase, "input: matched, queued for dispatch");
            let _ = self.tx.send(BindEvent {
                action: f.action,
                phase: f.phase,
                t_mono_ns: now_mono_ns(),
                template: f.template,
            });
        }
    }

    pub fn set_binds(&self, binds: Vec<BindSpec>) {
        let evs = self.lock().matcher.set_binds(binds);
        self.emit(evs);
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
        self.emit(evs);
        rx
    }

    pub fn stop_record(&self) {
        self.lock().recorder = None;
    }

    pub fn is_recording(&self) -> bool {
        self.lock().recorder.is_some()
    }

    /// Feed one native event. Returns true when the native event must be swallowed.
    pub fn on_raw(&self, raw: Raw) -> bool {
        let mut guard = self.lock();
        let g = &mut *guard;
        if let Some(rec) = g.recorder.as_mut() {
            let (swallow, done) = Self::record_step(rec, raw);
            // Keep matcher modifier state in sync; binds stay suspended (events dropped).
            if matches!(raw, Raw::Mods(_) | Raw::Reset) {
                g.matcher.handle(raw);
            }
            if let Some(t) = done {
                if let Some(rec) = g.recorder.take() {
                    let _ = rec.tx.send(t);
                    g.pending_up = (rec.swallow_up_keys, rec.swallow_up_mouse);
                }
            }
            return swallow;
        }
        if let Some(s) = take_pending(&mut g.pending_up, &raw) {
            return s;
        }
        let out = g.matcher.handle(raw);
        drop(guard);
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
}
