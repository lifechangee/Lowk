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
    /// G6: takes mouse events only (hold-Ctrl): never key, never activates, so the app below
    /// keeps keyboard focus. Release returns to `Passive`.
    Hover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeInput {
    /// `Passive` / `Interactive`; `Moving` is treated as [`ModeInput::MoveDown`].
    Set(OverlayMode),
    MoveDown,
    MoveUp,
    /// G6: the click-hold key went down / up.
    HoverDown,
    HoverUp,
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
    hover: bool,
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
            hover: false,
        }
    }

    /// What the UI sees.
    pub fn effective(&self) -> OverlayMode {
        if self.moving {
            OverlayMode::Moving
        } else if self.base == Interaction::Interactive {
            OverlayMode::Interactive
        } else if self.hover {
            OverlayMode::Hover
        } else {
            OverlayMode::Passive
        }
    }

    /// What the native window style must follow. Typing mode (Interactive) outranks the
    /// hold-to-click state; `Moving` never touches the native style.
    pub fn style(&self) -> Interaction {
        if self.base == Interaction::Interactive {
            Interaction::Interactive
        } else if self.hover {
            Interaction::Hover
        } else {
            Interaction::Passive
        }
    }

    pub fn is_moving(&self) -> bool {
        self.moving
    }

    pub fn apply(&mut self, input: ModeInput) -> Option<ModeChange> {
        let prev = self.effective();
        let prev_style = self.style();
        match input {
            ModeInput::Set(OverlayMode::Moving) | ModeInput::MoveDown => self.moving = true,
            ModeInput::Set(OverlayMode::Passive) => self.base = Interaction::Passive,
            ModeInput::Set(OverlayMode::Interactive) => self.base = Interaction::Interactive,
            // Hover is driven by the hold bind only; as a `Set` it is the hold going down.
            ModeInput::Set(OverlayMode::Hover) | ModeInput::HoverDown => self.hover = true,
            ModeInput::MoveUp => self.moving = false,
            ModeInput::HoverUp => self.hover = false,
            ModeInput::Blur | ModeInput::Escape => self.base = Interaction::Passive,
            ModeInput::Hidden => {
                self.base = Interaction::Passive;
                self.hover = false;
            }
        }
        let now = self.effective();
        let style_changed = prev_style != self.style();
        (prev != now || style_changed).then_some(ModeChange {
            prev,
            now,
            style_changed,
        })
    }

    /// Undo a base-style change whose native application failed.
    pub fn force_style(&mut self, style: Interaction) {
        self.hover = style == Interaction::Hover;
        self.base = if style == Interaction::Interactive {
            Interaction::Interactive
        } else {
            Interaction::Passive
        };
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
    fn hover_hold_toggles_style_without_touching_base() {
        let mut m = ModeMachine::new();
        let c = m.apply(ModeInput::HoverDown).unwrap();
        assert_eq!((c.now, c.style_changed), (Hover, true));
        assert_eq!(m.style(), Interaction::Hover);
        assert!(m.apply(ModeInput::HoverDown).is_none());
        let c = m.apply(ModeInput::HoverUp).unwrap();
        assert_eq!((c.now, c.style_changed), (Passive, true));
        assert_eq!(m.style(), Interaction::Passive);
    }

    #[test]
    fn typing_mode_outranks_hover_and_move_outranks_both() {
        let mut m = ModeMachine::new();
        m.apply(ModeInput::HoverDown);
        let c = m.apply(ModeInput::Set(Interactive)).unwrap();
        assert_eq!((c.now, c.style_changed), (Interactive, true));
        // releasing the hold while typing changes nothing visible
        assert!(m.apply(ModeInput::HoverUp).is_none());
        let mut m = ModeMachine::new();
        m.apply(ModeInput::HoverDown);
        let c = m.apply(ModeInput::MoveDown).unwrap();
        assert_eq!((c.now, c.style_changed), (Moving, false));
        assert_eq!(m.apply(ModeInput::MoveUp).unwrap().now, Hover);
    }

    #[test]
    fn hidden_clears_hover() {
        let mut m = ModeMachine::new();
        m.apply(ModeInput::HoverDown);
        assert_eq!(m.apply(ModeInput::Hidden).unwrap().now, Passive);
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

    /// The main window taking focus (Blur) must not drop a running hover hold, and the hold
    /// keeps the overlay in the Hover style (clickable, not click-through).
    #[test]
    fn hover_survives_main_window_focus_changes() {
        let mut m = ModeMachine::new();
        m.apply(ModeInput::HoverDown);
        assert_eq!(m.style(), Interaction::Hover);
        assert!(m.apply(ModeInput::Blur).is_none());
        assert_eq!(m.style(), Interaction::Hover);
        assert_eq!(m.effective(), Hover);
        assert!(m.apply(ModeInput::HoverUp).is_some());
        assert_eq!(m.style(), Interaction::Passive);
    }
}
