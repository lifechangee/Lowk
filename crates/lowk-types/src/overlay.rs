//! Overlay / stealth DTOs.
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OverlayMode {
    /// Click-through, no focus. Default.
    Passive,
    /// Takes clicks and keyboard; auto-exits on blur or Esc.
    Interactive,
    /// Hold-to-move magnet; returns to the prior mode on release.
    Moving,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum StealthStatus {
    Verified,
    Degraded(String),
    Unavailable(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ToneKind {
    Dark,
    Light,
    Color,
}

/// `overlay://tone` payload: inputs of the adaptive CSS tokens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Tone {
    pub tone: ToneKind,
    pub rgb: String,
    pub fg: String,
    pub inv: String,
    pub int: String,
    pub mv: String,
    pub luma: f32,
    pub busy: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum OverlayEvent {
    Mode(OverlayMode),
    Tone(Tone),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SelfTestReport {
    pub overlay_hidden: bool,
    pub main_visible: bool,
    pub details: String,
}

/// Rectangle in global top-left-origin points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
