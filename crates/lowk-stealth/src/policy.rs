//! One window policy per label, defined in one place. Never shared state
//! between windows: the overlay's policy cannot be changed by anything the
//! main window does (predecessor failure: both windows shared one flag).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowRole {
    Overlay,
    Main,
    /// The unobtrusive edge strip ("pill" visibility option). Stealth like the overlay.
    Pill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowPolicy {
    pub role: WindowRole,
    /// Excluded from screen capture / sharing.
    pub capture_excluded: bool,
    /// Absent from Dock / taskbar / Alt+Tab / Cmd+Tab.
    pub hidden_from_switchers: bool,
    /// May activate the app / take focus when shown.
    pub activates: bool,
}

impl WindowPolicy {
    pub const OVERLAY: WindowPolicy = WindowPolicy {
        role: WindowRole::Overlay,
        capture_excluded: true,
        hidden_from_switchers: true,
        activates: false,
    };
    pub const PILL: WindowPolicy = WindowPolicy {
        role: WindowRole::Pill,
        capture_excluded: true,
        hidden_from_switchers: true,
        activates: false,
    };
    pub fn main(hide_main: bool) -> WindowPolicy {
        WindowPolicy {
            role: WindowRole::Main,
            capture_excluded: hide_main,
            hidden_from_switchers: false,
            activates: true,
        }
    }
    /// True for windows that must fail closed and obey panic hide.
    pub fn is_stealth(&self) -> bool {
        self.capture_excluded && self.hidden_from_switchers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlay_and_pill_are_always_stealth() {
        assert!(WindowPolicy::OVERLAY.is_stealth());
        assert!(WindowPolicy::PILL.is_stealth());
        const { assert!(!WindowPolicy::OVERLAY.activates) };
    }
    #[test]
    fn main_policy_is_independent() {
        assert!(!WindowPolicy::main(false).capture_excluded);
        assert!(WindowPolicy::main(true).capture_excluded);
        assert!(!WindowPolicy::main(true).is_stealth());
        const { assert!(WindowPolicy::OVERLAY.capture_excluded) };
    }
}
