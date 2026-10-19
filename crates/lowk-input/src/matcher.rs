//! Platform independent bind state machine. Backends translate native events
//! into [`Raw`]; the matcher decides which binds fire and whether the native
//! event must be swallowed. Pure and unit-tested.
use lowk_types::{ActionId, BindSpec, Mods, MouseBtn, Phase, Trigger};
use std::collections::HashSet;

/// Normalised native input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raw {
    /// Non-modifier key. `code` is the W3C name or `"Unknown"`.
    Key {
        code: &'static str,
        down: bool,
        repeat: bool,
    },
    /// Complete current modifier state.
    Mods(Mods),
    Mouse {
        button: MouseBtn,
        down: bool,
    },
    /// Hook was disabled/re-enabled: forget held state, release everything.
    Reset,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub swallow: bool,
    pub events: Vec<(ActionId, Phase)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    Key(&'static str),
    Mouse(MouseBtn),
    Mods,
}

#[derive(Default)]
pub struct Matcher {
    binds: Vec<BindSpec>,
    mods: Mods,
    keys_down: HashSet<&'static str>,
    mouse_down: u32,
    active: Vec<(usize, Held)>,
}

impl Matcher {
    /// Replace the table; returns `Up` events for anything held under the old one.
    pub fn set_binds(&mut self, binds: Vec<BindSpec>) -> Vec<(ActionId, Phase)> {
        let out = self.release_all();
        self.binds = binds;
        out
    }

    pub fn release_all(&mut self) -> Vec<(ActionId, Phase)> {
        let out = self
            .active
            .drain(..)
            .map(|(i, _)| (self.binds[i].action, Phase::Up))
            .collect();
        self.keys_down.clear();
        self.mouse_down = 0;
        out
    }

    fn is_active(&self, idx: usize) -> bool {
        self.active.iter().any(|(i, _)| *i == idx)
    }

    /// End actives for which `pred` holds, pushing `Up`. When `swallow_up`, a
    /// swallowing bind also swallows the release.
    fn end(&mut self, out: &mut Outcome, swallow_up: bool, pred: impl Fn(&BindSpec, Held) -> bool) {
        let binds = &self.binds;
        self.active.retain(|&(i, h)| {
            if pred(&binds[i], h) {
                out.events.push((binds[i].action, Phase::Up));
                out.swallow |= swallow_up && binds[i].swallow;
                false
            } else {
                true
            }
        });
    }

    pub fn handle(&mut self, raw: Raw) -> Outcome {
        let mut out = Outcome::default();
        match raw {
            Raw::Reset => {
                out.events = self.release_all();
                self.mods = Mods::NONE;
            }
            Raw::Key {
                code,
                down: true,
                repeat,
            } => {
                if repeat {
                    // Keep swallowing auto-repeat of a swallowed combo; no event.
                    out.swallow = self
                        .active
                        .iter()
                        .any(|&(i, h)| h == Held::Key(code) && self.binds[i].swallow);
                    return out;
                }
                self.keys_down.insert(code);
                // A real key while a modifier-only bind is held (Alt+Tab) cancels it.
                self.end(&mut out, false, |_, h| h == Held::Mods);
                for i in 0..self.binds.len() {
                    if let Trigger::Combo { mods, key } = &self.binds[i].trigger {
                        if key.0 == code && *mods == self.mods && !self.is_active(i) {
                            out.events.push((self.binds[i].action, Phase::Down));
                            out.swallow |= self.binds[i].swallow;
                            self.active.push((i, Held::Key(code)));
                        }
                    }
                }
            }
            Raw::Key {
                code, down: false, ..
            } => {
                self.keys_down.remove(code);
                self.end(&mut out, true, |_, h| h == Held::Key(code));
            }
            Raw::Mods(m) => {
                if m == self.mods {
                    return out;
                }
                self.mods = m;
                self.end(&mut out, false, |b, h| match (h, &b.trigger) {
                    (Held::Key(_), Trigger::Combo { mods, .. }) => *mods != m,
                    (Held::Mods, Trigger::ModifierOnly { mods }) => *mods != m,
                    _ => false,
                });
                if self.keys_down.is_empty() && self.mouse_down == 0 && m != Mods::NONE {
                    for i in 0..self.binds.len() {
                        if let Trigger::ModifierOnly { mods } = &self.binds[i].trigger {
                            if *mods == m && !self.is_active(i) {
                                out.events.push((self.binds[i].action, Phase::Down));
                                self.active.push((i, Held::Mods));
                            }
                        }
                    }
                }
            }
            Raw::Mouse { button, down: true } => {
                self.mouse_down += 1;
                for i in 0..self.binds.len() {
                    if let Trigger::Mouse { button: b, mods } = &self.binds[i].trigger {
                        if *b == button && *mods == self.mods && !self.is_active(i) {
                            out.events.push((self.binds[i].action, Phase::Down));
                            out.swallow |= self.binds[i].swallow;
                            self.active.push((i, Held::Mouse(button)));
                        }
                    }
                }
            }
            Raw::Mouse { button, down: false } => {
                self.mouse_down = self.mouse_down.saturating_sub(1);
                self.end(&mut out, true, |_, h| h == Held::Mouse(button));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::KeyCode;

    fn combo(a: ActionId, mods: Mods, key: &str, swallow: bool) -> BindSpec {
        BindSpec {
            action: a,
            trigger: Trigger::Combo {
                mods,
                key: KeyCode::new(key),
            },
            swallow,
        }
    }
    fn kd(code: &'static str) -> Raw {
        Raw::Key {
            code,
            down: true,
            repeat: false,
        }
    }
    fn ku(code: &'static str) -> Raw {
        Raw::Key {
            code,
            down: false,
            repeat: false,
        }
    }
    fn m(binds: Vec<BindSpec>) -> Matcher {
        let mut x = Matcher::default();
        x.set_binds(binds);
        x
    }
    fn alt_hold() -> BindSpec {
        BindSpec {
            action: ActionId::MoveHold,
            trigger: Trigger::ModifierOnly { mods: Mods::ALT },
            swallow: false,
        }
    }

    #[test]
    fn combo_down_up_and_exact_mods() {
        let mut x = m(vec![combo(ActionId::Ask, Mods::META, "Enter", true)]);
        assert_eq!(x.handle(kd("Enter")), Outcome::default());
        x.handle(ku("Enter"));
        x.handle(Raw::Mods(Mods::META));
        let o = x.handle(kd("Enter"));
        assert_eq!(o.events, vec![(ActionId::Ask, Phase::Down)]);
        assert!(o.swallow);
        let o = x.handle(Raw::Key {
            code: "Enter",
            down: true,
            repeat: true,
        });
        assert!(o.swallow && o.events.is_empty());
        let o = x.handle(ku("Enter"));
        assert_eq!(o.events, vec![(ActionId::Ask, Phase::Up)]);
        assert!(o.swallow);
        x.handle(Raw::Mods(Mods::META_SHIFT));
        assert!(x.handle(kd("Enter")).events.is_empty());
    }

    #[test]
    fn combo_ends_when_modifier_released_first() {
        let mut x = m(vec![combo(ActionId::Ask, Mods::META, "KeyA", false)]);
        x.handle(Raw::Mods(Mods::META));
        x.handle(kd("KeyA"));
        let o = x.handle(Raw::Mods(Mods::NONE));
        assert_eq!(o.events, vec![(ActionId::Ask, Phase::Up)]);
        assert!(x.handle(ku("KeyA")).events.is_empty());
    }

    #[test]
    fn modifier_only_hold() {
        let mut x = m(vec![alt_hold()]);
        let o = x.handle(Raw::Mods(Mods::ALT));
        assert_eq!(o.events, vec![(ActionId::MoveHold, Phase::Down)]);
        assert!(!o.swallow);
        let o = x.handle(Raw::Mods(Mods::NONE));
        assert_eq!(o.events, vec![(ActionId::MoveHold, Phase::Up)]);
    }

    #[test]
    fn modifier_only_cancelled_by_key_and_not_retriggered() {
        let mut x = m(vec![alt_hold()]);
        x.handle(Raw::Mods(Mods::ALT));
        let o = x.handle(kd("Tab"));
        assert_eq!(o.events, vec![(ActionId::MoveHold, Phase::Up)]);
        x.handle(ku("Tab"));
        x.handle(Raw::Mods(Mods::NONE));
        assert_eq!(x.handle(Raw::Mods(Mods::ALT)).events.len(), 1);
    }

    #[test]
    fn modifier_only_not_started_when_key_already_down() {
        let mut x = m(vec![alt_hold()]);
        x.handle(kd("KeyA"));
        assert!(x.handle(Raw::Mods(Mods::ALT)).events.is_empty());
    }

    #[test]
    fn mouse_button_swallow_down_and_up() {
        let mut x = m(vec![BindSpec {
            action: ActionId::Ask,
            trigger: Trigger::Mouse {
                button: MouseBtn::Back,
                mods: Mods::NONE,
            },
            swallow: true,
        }]);
        let d = x.handle(Raw::Mouse {
            button: MouseBtn::Back,
            down: true,
        });
        assert!(d.swallow);
        assert_eq!(d.events, vec![(ActionId::Ask, Phase::Down)]);
        let u = x.handle(Raw::Mouse {
            button: MouseBtn::Back,
            down: false,
        });
        assert!(u.swallow);
        assert_eq!(u.events, vec![(ActionId::Ask, Phase::Up)]);
        let o = x.handle(Raw::Mouse {
            button: MouseBtn::Forward,
            down: true,
        });
        assert_eq!(o, Outcome::default());
    }

    #[test]
    fn reset_and_set_binds_release_held() {
        let mut x = m(vec![alt_hold()]);
        x.handle(Raw::Mods(Mods::ALT));
        let o = x.handle(Raw::Reset);
        assert_eq!(o.events, vec![(ActionId::MoveHold, Phase::Up)]);
        x.handle(Raw::Mods(Mods::ALT));
        assert_eq!(x.set_binds(vec![]), vec![(ActionId::MoveHold, Phase::Up)]);
    }
}
