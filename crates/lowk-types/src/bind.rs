//! Input binds (hotkeys, hold-binds, mouse buttons).
use crate::MonoNs;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ActionId {
    ToggleRecord,
    Ask,
    Screenshot,
    ScreenshotArea,
    ToggleInteractive,
    ToggleOverlay,
    MoveHold,
    PanicHide,
    ToggleMain,
    NextTemplate,
    PrevTemplate,
    CancelAnswer,
    ClearShots,
    /// G6: hold = the overlay is clickable without taking focus (default Ctrl).
    ClickHold,
    /// G6: ask with a fixed template, no overlay mode switch needed.
    AskBrief,
    AskComplement,
    AskSay,
    AskFactcheck,
    AskFollowup,
    AskRecap,
}

impl ActionId {
    /// The template an `Ask*` action sends with (`None` for the others and for plain `Ask`,
    /// which uses the template selected in the overlay chip).
    pub fn template_id(self) -> Option<&'static str> {
        Some(match self {
            ActionId::AskBrief => "brief",
            ActionId::AskComplement => "complement",
            ActionId::AskSay => "say",
            ActionId::AskFactcheck => "factcheck",
            ActionId::AskFollowup => "followup",
            ActionId::AskRecap => "recap",
            _ => return None,
        })
    }
}

/// Modifier set. `meta` is Command on macOS and the Windows key on Windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct Mods {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub meta: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };
    pub const CTRL: Mods = Mods {
        ctrl: true,
        ..Mods::NONE
    };
    pub const ALT: Mods = Mods {
        alt: true,
        ..Mods::NONE
    };
    pub const META: Mods = Mods {
        meta: true,
        ..Mods::NONE
    };
    pub const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        ..Mods::NONE
    };
    pub const META_SHIFT: Mods = Mods {
        meta: true,
        shift: true,
        ..Mods::NONE
    };
}

/// W3C `KeyboardEvent.code` name (`KeyS`, `Enter`, `Backslash`, ...): layout
/// independent and identical across OS and the UI bind recorder.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct KeyCode(pub String);

impl KeyCode {
    pub fn new(code: &str) -> Self {
        Self(code.to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "n", rename_all = "snake_case")]
pub enum MouseBtn {
    Middle,
    Back,
    Forward,
    Other(u8),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    Combo { mods: Mods, key: KeyCode },
    ModifierOnly { mods: Mods },
    Mouse { button: MouseBtn, mods: Mods },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Down,
    Up,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BindSpec {
    pub action: ActionId,
    pub trigger: Trigger,
    /// Swallow the event so other apps do not see it.
    #[serde(default)]
    pub swallow: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BindEvent {
    pub action: ActionId,
    pub phase: Phase,
    pub t_mono_ns: MonoNs,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum BindStatus {
    Active,
    /// A permission is missing (listen-only or fallback).
    Degraded(crate::PermKind),
    /// Known OS shortcut conflict; text is the English shortcut name.
    Conflict(String),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BindStatusEntry {
    pub action: ActionId,
    pub status: BindStatus,
}

/// Result of `InputBackend::set_binds`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BindReport {
    pub entries: Vec<BindStatusEntry>,
}

/// `binds_get` / `binds_set` payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BindsState {
    pub binds: Vec<BindSpec>,
    pub report: BindReport,
}
