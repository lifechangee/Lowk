//! OS permissions.
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PermKind {
    Microphone,
    SystemAudio,
    ScreenRecording,
    InputMonitoring,
    Accessibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PermState {
    Granted,
    Denied,
    NotDetermined,
    /// No public preflight exists (e.g. macOS system audio): probe instead.
    Unknown,
    /// The permission does not exist on this OS.
    NotApplicable,
}
