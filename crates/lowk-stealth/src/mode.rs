//! Overlay mode machine: pure state, no OS calls.
//!
//! Focus is separate from stealth: stealth flags never depend on the mode.
//! The machine tracks a *base* mode (`Passive` or `Interactive`, the thing the
//! native window style follows) and a *moving* flag (hold-to-move magnet).
//! `Moving` never touches the native style, so releasing the bind restores
//! the prior mode exactly and focus does not churn while the overlay follows
//! the cursor.

use lowk_types::OverlayMode;

/// Native interaction style derived from the base mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interaction {
    /// Click-through, never key, never activates.
    Passive,
    /// Takes clicks and keyboard (key window / foreground), app not activated.
    Interactive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeInput {
    /// `Passive` / `Interactive`; `Moving` is treated as [`ModeInput::MoveDown`].
    Set(OverlayMode),
    MoveDown,
    MoveUp,
    /// The overlay lost key/foreground status while interactive.
    Blur,
    /// Escape pressed in the interactive overlay.
    Escape,
    /// The overlay was hidden (interactive cannot survive hiding).
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeChange {
    pub prev: OverlayMode,
    pub now: OverlayMode,
    /// The native style must be re-synced (base mode changed).
    pub style_changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeMachine {
    base: Interaction,
    moving: bool,
}

impl Default for ModeMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl ModeMachine {
    pub const fn new() -> Self {
        Self {
            base: Interaction::Passive,
            moving: false,
        }
    }

    /// What the UI sees.
    pub fn effective(&self) -> OverlayMode {
        if self.moving {
            OverlayMode::Moving
        } else {
            match self.base {
                Interaction::Passive => OverlayMode::Passive,
                Interaction::Interactive => OverlayMode::Interactive,
            }
        }
    }

    /// What the native window style must follow.
    pub fn style(&self) -> Interaction {
        self.base
    }

    pub fn is_moving(&self) -> bool {
        self.moving
    }

    pub fn apply(&mut self, input: ModeInput) -> Option<ModeChange> {
        let prev = self.effective();
        let prev_base = self.base;
        match input {
            ModeInput::Set(OverlayMode::Moving) | ModeInput::MoveDown => self.moving = true,
            ModeInput::Set(OverlayMode::Passive) => self.base = Interaction::Passive,
            ModeInput::Set(OverlayMode::Interactive) => self.base = Interaction::Interactive,
            ModeInput::MoveUp => self.moving = false,
            ModeInput::Blur | ModeInput::Escape | ModeInput::Hidden => self.base = Interaction::Passive,
        }
        let now = self.effective();
        let style_changed = prev_base != self.base;
        (prev != now || style_changed).then_some(ModeChange {
            prev,
            now,
            style_changed,
        })
    }

    /// Undo a base-style change whose native application failed.
    pub fn force_style(&mut self, style: Interaction) {
        self.base = style;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use OverlayMode::*;

    #[test]
    fn default_is_passive() {
        let m = ModeMachine::new();
        assert_eq!(m.effective(), Passive);
        assert_eq!(m.style(), Interaction::Passive);
    }

    #[test]
    fn toggle_interactive() {
        let mut m = ModeMachine::new();
        let c = m.apply(ModeInput::Set(Interactive)).unwrap();
        assert_eq!((c.prev, c.now, c.style_changed), (Passive, Interactive, true));
        assert!(m.apply(ModeInput::Set(Interactive)).is_none(), "idempotent");
        let c = m.apply(ModeInput::Set(Passive)).unwrap();
        assert_eq!(c.now, Passive);
    }

    #[test]
    fn moving_returns_to_prior_mode_from_passive() {
        let mut m = ModeMachine::new();
        let c = m.apply(ModeInput::MoveDown).unwrap();
        assert_eq!((c.now, c.style_changed), (Moving, false));
        assert!(m.apply(ModeInput::MoveDown).is_none());
        let c = m.apply(ModeInput::MoveUp).unwrap();
        assert_eq!(c.now, Passive);
    }

    #[test]
    fn moving_returns_to_prior_mode_from_interactive() {
        let mut m = ModeMachine::new();
        m.apply(ModeInput::Set(Interactive));
        let c = m.apply(ModeInput::MoveDown).unwrap();
        assert_eq!(c.now, Moving);
        assert!(!c.style_changed, "moving must not churn native focus style");
        assert_eq!(m.style(), Interaction::Interactive);
        let c = m.apply(ModeInput::MoveUp).unwrap();
        assert_eq!(c.now, Interactive);
    }

    #[test]
    fn set_while_moving_applies_on_release() {
        let mut m = ModeMachine::new();
        m.apply(ModeInput::MoveDown);
        let c = m.apply(ModeInput::Set(Interactive)).unwrap();
        assert!(c.style_changed);
        assert_eq!(m.effective(), Moving);
        assert_eq!(m.apply(ModeInput::MoveUp).unwrap().now, Interactive);
    }

    #[test]
    fn set_moving_is_move_down() {
        let mut m = ModeMachine::new();
        m.apply(ModeInput::Set(Moving));
        assert!(m.is_moving());
    }

    #[test]
    fn blur_escape_hidden_leave_interactive() {
        for i in [ModeInput::Blur, ModeInput::Escape, ModeInput::Hidden] {
            let mut m = ModeMachine::new();
            m.apply(ModeInput::Set(Interactive));
            let c = m.apply(i).unwrap();
            assert_eq!(c.now, Passive);
            assert!(c.style_changed);
            assert!(m.apply(i).is_none(), "no-op when already passive");
        }
    }

    #[test]
    fn blur_while_moving_defers_visible_change() {
        let mut m = ModeMachine::new();
        m.apply(ModeInput::Set(Interactive));
        m.apply(ModeInput::MoveDown);
        let c = m.apply(ModeInput::Blur).unwrap();
        assert_eq!((c.prev, c.now, c.style_changed), (Moving, Moving, true));
        assert_eq!(m.apply(ModeInput::MoveUp).unwrap().now, Passive);
    }
}
