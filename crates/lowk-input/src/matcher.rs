//! Platform independent bind state machine. Backends translate native events
//! into [`Raw`]; the matcher decides which binds fire and whether the native
//! event must be swallowed. Pure and unit-tested.
use lowk_types::{ActionId, BindSpec, Mods, MouseBtn, Phase, Trigger};
use std::collections::HashSet;
use std::time::{Duration, Instant};

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

/// One bind firing (`template` is set for `ActionId::AskTemplate`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fire {
    pub action: ActionId,
    pub phase: Phase,
    pub template: Option<String>,
}

impl Fire {
    fn of(b: &BindSpec, phase: Phase) -> Self {
        Fire {
            action: b.action,
            phase,
            template: b.template.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub swallow: bool,
    pub events: Vec<Fire>,
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
    hold_delay: Duration,
    pending_mods: Option<PendingMods>,
}

#[derive(Clone, Debug)]
struct PendingMods {
    indices: Vec<usize>,
    mods: Mods,
    since: Instant,
}

impl Matcher {
    /// Modifier-only binds that hold for the modifier state `m`: those whose mods equal `m`; when
    /// none does, a `MoveHold` bind wins if `m` is fully explained by modifier-only binds (G6:
    /// holding Ctrl for click-through-off and then Alt must start the move, not drop both).
    fn wanted_mod_only(&self, m: Mods) -> Vec<usize> {
        let only = |i: &usize| matches!(self.binds[*i].trigger, Trigger::ModifierOnly { .. });
        let mods_of = |i: usize| match &self.binds[i].trigger {
            Trigger::ModifierOnly { mods } => *mods,
            _ => Mods::NONE,
        };
        let exact: Vec<usize> = (0..self.binds.len())
            .filter(only)
            .filter(|&i| mods_of(i) == m)
            .collect();
        if !exact.is_empty() || m == Mods::NONE {
            return exact;
        }
        let sub = |a: Mods, b: Mods| {
            (!a.ctrl || b.ctrl) && (!a.alt || b.alt) && (!a.shift || b.shift) && (!a.meta || b.meta)
        };
        let cover: Vec<usize> = (0..self.binds.len())
            .filter(only)
            .filter(|&i| sub(mods_of(i), m))
            .collect();
        let union = cover.iter().fold(Mods::NONE, |u, &i| {
            let x = mods_of(i);
            Mods {
                ctrl: u.ctrl || x.ctrl,
                alt: u.alt || x.alt,
                shift: u.shift || x.shift,
                meta: u.meta || x.meta,
            }
        });
        if union == m {
            if let Some(&mv) = cover
                .iter()
                .find(|&&i| self.binds[i].action == ActionId::MoveHold)
            {
                return vec![mv];
            }
        }
        vec![]
    }

    /// Replace the table; returns `Up` events for anything held under the old one.
    pub fn set_binds(&mut self, binds: Vec<BindSpec>) -> Vec<Fire> {
        let out = self.release_all();
        self.binds = binds;
        out
    }

    pub fn release_all(&mut self) -> Vec<Fire> {
        self.cancel_pending("reset");
        let out = self
            .active
            .drain(..)
            .map(|(i, _)| Fire::of(&self.binds[i], Phase::Up))
            .collect();
        self.keys_down.clear();
        self.mouse_down = 0;
        out
    }

    /// Adopt the OS modifier state without firing anything: a modifier already held when the hook was
    /// (re)created is not a press. The next release and fresh press behave normally.
    pub fn set_baseline_mods(&mut self, m: Mods) {
        if m != self.mods {
            self.cancel_pending("mods changed");
        }
        self.mods = m;
    }

    /// Keys the matcher believes are down (for the health check; never logged).
    pub fn held_keys(&self) -> Vec<&'static str> {
        self.keys_down.iter().copied().collect()
    }

    /// Count of mouse buttons believed down.
    pub fn mouse_held(&self) -> u32 {
        self.mouse_down
    }

    /// A mouse-up never reached us: forget the count and end binds held by a button.
    pub fn release_stale_mouse(&mut self) -> Vec<Fire> {
        let mut out = Outcome::default();
        self.mouse_down = 0;
        self.end(&mut out, false, |_, h| matches!(h, Held::Mouse(_)));
        out.events
    }

    pub fn set_hold_delay(&mut self, delay: Duration) -> Outcome {
        self.hold_delay = delay;
        if delay.is_zero() {
            if let Some(pending) = self.pending_mods.take() {
                let out = self.fire_mods(pending.indices);
                if !out.events.is_empty() {
                    tracing::debug!("input: modifier-only bind pending fired");
                }
                return out;
            }
        }
        Outcome::default()
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending_mods.as_ref().map(|p| p.since + self.hold_delay)
    }

    fn cancel_pending(&mut self, reason: &'static str) {
        if self.pending_mods.take().is_some() {
            tracing::debug!(reason, "input: modifier-only bind pending cancelled");
        }
    }

    fn arm_mods(&mut self, m: Mods, now: Instant) {
        if m == Mods::NONE || !self.keys_down.is_empty() || self.mouse_down != 0 {
            self.cancel_pending(if m == Mods::NONE { "released" } else { "key" });
            return;
        }
        let indices = self.wanted_mod_only(m);
        if indices.is_empty() {
            self.cancel_pending("mods changed");
            return;
        }
        self.pending_mods = Some(PendingMods {
            indices,
            mods: m,
            since: now,
        });
        tracing::debug!(mods = ?m, "input: modifier-only bind pending armed");
    }

    fn fire_mods(&mut self, indices: Vec<usize>) -> Outcome {
        let mut out = Outcome::default();
        for i in indices {
            if !self.is_active(i) {
                out.events.push(Fire::of(&self.binds[i], Phase::Down));
                self.active.push((i, Held::Mods));
            }
        }
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
                out.events.push(Fire::of(&binds[i], Phase::Up));
                out.swallow |= swallow_up && binds[i].swallow;
                false
            } else {
                true
            }
        });
    }

    pub fn handle(&mut self, raw: Raw) -> Outcome {
        self.handle_at(raw, Instant::now())
    }

    pub fn handle_at(&mut self, raw: Raw, now: Instant) -> Outcome {
        let mut out = Outcome::default();
        match raw {
            Raw::Reset => {
                self.cancel_pending("reset");
                out.events = self.release_all();
                self.mods = Mods::NONE;
            }
            Raw::Key {
                code,
                down: true,
                repeat,
            } => {
                self.cancel_pending("key");
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
                            out.events.push(Fire::of(&self.binds[i], Phase::Down));
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
                let wanted = self.wanted_mod_only(m);
                // A hold already fired proves intent: escalating it (Ctrl-hold, then +Alt) switches at once.
                let holding = self.active.iter().any(|(_, h)| *h == Held::Mods);
                if self.pending_mods.is_some() && self.pending_mods.as_ref().is_some_and(|p| p.mods != m) {
                    self.cancel_pending(if m == Mods::NONE {
                        "released"
                    } else {
                        "mods changed"
                    });
                }
                let wanted_t: Vec<Trigger> = wanted.iter().map(|&i| self.binds[i].trigger.clone()).collect();
                self.end(&mut out, false, |b, h| match (h, &b.trigger) {
                    (Held::Key(_), Trigger::Combo { mods, .. }) => *mods != m,
                    (Held::Mods, Trigger::ModifierOnly { .. }) => !wanted_t.contains(&b.trigger),
                    _ => false,
                });
                if self.keys_down.is_empty() && self.mouse_down == 0 && m != Mods::NONE {
                    if self.hold_delay.is_zero() || holding {
                        out.events.extend(self.fire_mods(wanted).events);
                    } else if !wanted.is_empty() {
                        self.arm_mods(m, now);
                    }
                }
            }
            Raw::Mouse { button, down: true } => {
                self.cancel_pending("mouse");
                self.mouse_down += 1;
                for i in 0..self.binds.len() {
                    if let Trigger::Mouse { button: b, mods } = &self.binds[i].trigger {
                        if *b == button && *mods == self.mods && !self.is_active(i) {
                            out.events.push(Fire::of(&self.binds[i], Phase::Down));
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

    pub fn poll(&mut self, now: Instant) -> Outcome {
        let Some(pending) = self.pending_mods.as_ref() else {
            return Outcome::default();
        };
        if now < pending.since + self.hold_delay {
            return Outcome::default();
        }
        let pending = self.pending_mods.take().expect("pending checked");
        if pending.mods != self.mods || !self.keys_down.is_empty() || self.mouse_down != 0 {
            return Outcome::default();
        }
        let out = self.fire_mods(pending.indices);
        if !out.events.is_empty() {
            tracing::debug!("input: modifier-only bind pending fired");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fire(action: ActionId, phase: Phase) -> Fire {
        Fire {
            action,
            phase,
            template: None,
        }
    }
    use lowk_types::KeyCode;
    use std::time::{Duration, Instant};

    fn combo(a: ActionId, mods: Mods, key: &str, swallow: bool) -> BindSpec {
        BindSpec {
            action: a,
            trigger: Trigger::Combo {
                mods,
                key: KeyCode::new(key),
            },
            swallow,
            template: None,
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
            template: None,
        }
    }

    #[test]
    fn combo_down_up_and_exact_mods() {
        let mut x = m(vec![combo(ActionId::Ask, Mods::META, "Enter", true)]);
        assert_eq!(x.handle(kd("Enter")), Outcome::default());
        x.handle(ku("Enter"));
        x.handle(Raw::Mods(Mods::META));
        let o = x.handle(kd("Enter"));
        assert_eq!(o.events, vec![fire(ActionId::Ask, Phase::Down)]);
        assert!(o.swallow);
        let o = x.handle(Raw::Key {
            code: "Enter",
            down: true,
            repeat: true,
        });
        assert!(o.swallow && o.events.is_empty());
        let o = x.handle(ku("Enter"));
        assert_eq!(o.events, vec![fire(ActionId::Ask, Phase::Up)]);
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
        assert_eq!(o.events, vec![fire(ActionId::Ask, Phase::Up)]);
        assert!(x.handle(ku("KeyA")).events.is_empty());
    }

    #[test]
    fn modifier_only_hold() {
        let mut x = m(vec![alt_hold()]);
        let o = x.handle(Raw::Mods(Mods::ALT));
        assert_eq!(o.events, vec![fire(ActionId::MoveHold, Phase::Down)]);
        assert!(!o.swallow);
        let o = x.handle(Raw::Mods(Mods::NONE));
        assert_eq!(o.events, vec![fire(ActionId::MoveHold, Phase::Up)]);
    }

    fn ctrl_hold() -> BindSpec {
        BindSpec {
            action: ActionId::ClickHold,
            trigger: Trigger::ModifierOnly { mods: Mods::CTRL },
            swallow: false,
            template: None,
        }
    }

    /// G6: Ctrl alone = ClickHold; Alt added = the move wins; Alt released = ClickHold again.
    #[test]
    fn move_wins_over_click_hold() {
        let mut x = m(vec![alt_hold(), ctrl_hold()]);
        let o = x.handle(Raw::Mods(Mods::CTRL));
        assert_eq!(o.events, vec![fire(ActionId::ClickHold, Phase::Down)]);
        let both = Mods {
            ctrl: true,
            alt: true,
            ..Mods::NONE
        };
        let o = x.handle(Raw::Mods(both));
        assert_eq!(
            o.events,
            vec![
                fire(ActionId::ClickHold, Phase::Up),
                fire(ActionId::MoveHold, Phase::Down)
            ]
        );
        let o = x.handle(Raw::Mods(Mods::CTRL));
        assert_eq!(
            o.events,
            vec![
                fire(ActionId::MoveHold, Phase::Up),
                fire(ActionId::ClickHold, Phase::Down)
            ]
        );
        let o = x.handle(Raw::Mods(Mods::NONE));
        assert_eq!(o.events, vec![fire(ActionId::ClickHold, Phase::Up)]);
        // Alt first, then Ctrl: the move keeps running.
        x.handle(Raw::Mods(Mods::ALT));
        let o = x.handle(Raw::Mods(both));
        assert!(o.events.is_empty(), "{:?}", o.events);
    }

    #[test]
    fn modifier_only_cancelled_by_key_and_not_retriggered() {
        let mut x = m(vec![alt_hold()]);
        x.handle(Raw::Mods(Mods::ALT));
        let o = x.handle(kd("Tab"));
        assert_eq!(o.events, vec![fire(ActionId::MoveHold, Phase::Up)]);
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
            template: None,
        }]);
        let d = x.handle(Raw::Mouse {
            button: MouseBtn::Back,
            down: true,
        });
        assert!(d.swallow);
        assert_eq!(d.events, vec![fire(ActionId::Ask, Phase::Down)]);
        let u = x.handle(Raw::Mouse {
            button: MouseBtn::Back,
            down: false,
        });
        assert!(u.swallow);
        assert_eq!(u.events, vec![fire(ActionId::Ask, Phase::Up)]);
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
        assert_eq!(o.events, vec![fire(ActionId::MoveHold, Phase::Up)]);
        x.handle(Raw::Mods(Mods::ALT));
        assert_eq!(x.set_binds(vec![]), vec![fire(ActionId::MoveHold, Phase::Up)]);
    }

    /// Owner's Windows test: after the overlay was toggled the hold binds (Ctrl = click-hold) were dead until
    /// the toggle combo was pressed again. A key whose key-up never arrived stays «down» in the matcher, and
    /// a modifier-only bind fires only while no other key is down. The platform layer now releases such a
    /// key (it compares with the OS), which is just a normal key-up for the matcher.
    #[test]
    fn a_lost_key_up_blocks_modifier_only_binds_until_it_is_released() {
        let mut x = m(vec![ctrl_hold(), alt_hold()]);
        // Ctrl+Shift+I toggles the overlay; the key-up of I is lost
        x.handle(Raw::Mods(Mods::CTRL));
        x.handle(Raw::Mods(Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        }));
        x.handle(kd("KeyI"));
        x.handle(Raw::Mods(Mods::NONE));
        // the stuck state: no hold binds fire
        let o = x.handle(Raw::Mods(Mods::CTRL));
        assert!(o.events.is_empty(), "stuck: {o:?}");
        x.handle(Raw::Mods(Mods::NONE));
        assert!(x.handle(Raw::Mods(Mods::ALT)).events.is_empty());
        x.handle(Raw::Mods(Mods::NONE));
        // the made-up key-up repairs it
        x.handle(ku("KeyI"));
        let o = x.handle(Raw::Mods(Mods::CTRL));
        assert_eq!(o.events, vec![fire(ActionId::ClickHold, Phase::Down)]);
        x.handle(Raw::Mods(Mods::NONE));
        let o = x.handle(Raw::Mods(Mods::ALT));
        assert_eq!(o.events, vec![fire(ActionId::MoveHold, Phase::Down)]);
    }

    /// Same for a mouse button whose release was lost (side buttons).
    #[test]
    fn a_lost_mouse_up_blocks_modifier_only_binds_until_it_is_released() {
        let mut x = m(vec![ctrl_hold()]);
        x.handle(Raw::Mouse {
            button: MouseBtn::Back,
            down: true,
        });
        assert!(x.handle(Raw::Mods(Mods::CTRL)).events.is_empty());
        x.handle(Raw::Mods(Mods::NONE));
        x.handle(Raw::Mouse {
            button: MouseBtn::Back,
            down: false,
        });
        let o = x.handle(Raw::Mods(Mods::CTRL));
        assert_eq!(o.events, vec![fire(ActionId::ClickHold, Phase::Down)]);
    }

    #[test]
    fn template_binds_fire_with_their_template_id() {
        let mut a = combo(ActionId::AskTemplate, Mods::META, "KeyJ", false);
        a.template = Some("my-tpl".into());
        let mut b = combo(ActionId::AskTemplate, Mods::META, "KeyK", false);
        b.template = Some("other".into());
        let mut x = m(vec![a, b]);
        x.handle(Raw::Mods(Mods::META));
        let o = x.handle(kd("KeyK"));
        assert_eq!(
            o.events,
            vec![Fire {
                action: ActionId::AskTemplate,
                phase: Phase::Down,
                template: Some("other".into())
            }]
        );
        let o = x.handle(kd("KeyJ"));
        assert_eq!(o.events[0].template.as_deref(), Some("my-tpl"));
    }

    fn delayed(binds: Vec<BindSpec>) -> Matcher {
        let mut x = m(binds);
        x.set_hold_delay(Duration::from_millis(1000));
        x
    }

    #[test]
    fn modifier_only_fires_at_deadline_and_release_pairs_after_fire() {
        let start = Instant::now();
        let mut x = delayed(vec![ctrl_hold()]);
        assert!(x.handle_at(Raw::Mods(Mods::CTRL), start).events.is_empty());
        assert!(x.poll(start + Duration::from_millis(999)).events.is_empty());
        assert_eq!(
            x.poll(start + Duration::from_millis(1000)).events,
            vec![fire(ActionId::ClickHold, Phase::Down)]
        );
        assert_eq!(
            x.handle_at(Raw::Mods(Mods::NONE), start + Duration::from_millis(1001))
                .events,
            vec![fire(ActionId::ClickHold, Phase::Up)]
        );
    }

    #[test]
    fn escalating_a_fired_hold_switches_at_once() {
        let start = Instant::now();
        let mut x = delayed(vec![ctrl_hold(), alt_hold()]);
        x.handle_at(Raw::Mods(Mods::CTRL), start);
        assert_eq!(
            x.poll(start + Duration::from_millis(1000)).events,
            vec![fire(ActionId::ClickHold, Phase::Down)]
        );
        let ctrl_alt = Mods {
            ctrl: true,
            alt: true,
            ..Mods::NONE
        };
        let o = x.handle_at(Raw::Mods(ctrl_alt), start + Duration::from_millis(1100));
        assert!(
            o.events.contains(&fire(ActionId::MoveHold, Phase::Down)),
            "{:?}",
            o.events
        );
        assert_eq!(x.next_deadline(), None, "no second wait");
    }

    #[test]
    fn key_during_delay_cancels_modifier_only_but_combo_still_fires() {
        let start = Instant::now();
        let mut x = delayed(vec![
            ctrl_hold(),
            combo(ActionId::Ask, Mods::CTRL, "Backslash", false),
        ]);
        x.handle_at(Raw::Mods(Mods::CTRL), start);
        let out = x.handle_at(kd("Backslash"), start + Duration::from_millis(50));
        assert_eq!(out.events, vec![fire(ActionId::Ask, Phase::Down)]);
        assert!(x.poll(start + Duration::from_secs(2)).events.is_empty());
    }

    #[test]
    fn mouse_cancels_pending_modifier_and_reset_clears_it() {
        let start = Instant::now();
        let mut x = delayed(vec![ctrl_hold()]);
        x.handle_at(Raw::Mods(Mods::CTRL), start);
        x.handle_at(
            Raw::Mouse {
                button: MouseBtn::Back,
                down: true,
            },
            start + Duration::from_millis(10),
        );
        assert!(x.poll(start + Duration::from_secs(2)).events.is_empty());
        x.handle_at(
            Raw::Mouse {
                button: MouseBtn::Back,
                down: false,
            },
            start + Duration::from_millis(20),
        );
        x.handle_at(Raw::Mods(Mods::NONE), start + Duration::from_millis(30));
        x.handle_at(Raw::Mods(Mods::CTRL), start + Duration::from_millis(40));
        x.handle_at(Raw::Reset, start + Duration::from_millis(50));
        assert!(x.poll(start + Duration::from_secs(2)).events.is_empty());
    }

    #[test]
    fn changed_modifiers_rearm_for_bind_wanted_by_new_mods() {
        let start = Instant::now();
        let both = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        let shift_hold = BindSpec {
            action: ActionId::MoveHold,
            trigger: Trigger::ModifierOnly { mods: both },
            swallow: false,
            template: None,
        };
        let mut x = delayed(vec![ctrl_hold(), shift_hold]);
        x.handle_at(Raw::Mods(Mods::CTRL), start);
        x.handle_at(Raw::Mods(both), start + Duration::from_millis(500));
        assert!(x.poll(start + Duration::from_millis(1499)).events.is_empty());
        assert_eq!(
            x.poll(start + Duration::from_millis(1500)).events,
            vec![fire(ActionId::MoveHold, Phase::Down)]
        );
    }

    #[test]
    fn pending_release_before_deadline_emits_nothing_and_zero_delay_is_immediate() {
        let start = Instant::now();
        let mut x = delayed(vec![ctrl_hold()]);
        x.handle_at(Raw::Mods(Mods::CTRL), start);
        assert!(x
            .handle_at(Raw::Mods(Mods::NONE), start + Duration::from_millis(999))
            .events
            .is_empty());
        x.set_hold_delay(Duration::ZERO);
        assert_eq!(
            x.handle_at(Raw::Mods(Mods::CTRL), start + Duration::from_secs(2))
                .events,
            vec![fire(ActionId::ClickHold, Phase::Down)]
        );
    }
}
