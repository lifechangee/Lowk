//! Speech-to-text contract.
use crate::time::MonoNs;
use crate::UttId;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;

/// Audio stream identity. UI labels: «Я» / «Система».
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    Me,
    System,
}

/// BCP-47-ish language tag, e.g. `ru`, `en`.
pub type LangTag = String;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SttKind {
    Partial,
    Final,
}

/// One revision of one utterance. `stt://segment` payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SttEvent {
    pub stream: Stream,
    pub utt: UttId,
    /// Revision of the same utterance (monotonic).
    pub rev: u32,
    pub kind: SttKind,
    pub text: String,
    pub lang: Option<LangTag>,
    pub t_start_ns: MonoNs,
    pub t_audio_end_ns: MonoNs,
    pub t_emit_ns: MonoNs,
}

/// Engine failure surfaced through the sink (isolated per stream).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SttError {
    pub stream: Stream,
    pub message: String,
    /// True when the engine host will restart the worker by itself.
    pub recoverable: bool,
}

/// Sink-based so cloud (websocket) engines and sidecars fit without blocking.
pub trait SttSink: Send + Sync {
    fn emit(&self, ev: SttEvent);
    fn error(&self, e: SttError);
}

impl<T: SttSink + ?Sized> SttSink for Arc<T> {
    fn emit(&self, ev: SttEvent) {
        (**self).emit(ev)
    }
    fn error(&self, e: SttError) {
        (**self).error(e)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    Local,
    Cloud,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StreamingMode {
    Native,
    Pseudo,
}

/// Data-driven description for the engine-selection UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct EngineDescriptor {
    pub id: String,
    pub name: String,
    pub kind: EngineKind,
    pub languages: Vec<LangTag>,
    pub streaming: StreamingMode,
    /// Download size in MB (0 for cloud).
    pub size_mb: u32,
    pub license: String,
}

/// Static pre-filter result of `SttEngineFactory::fitness`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum Fitness {
    Recommended,
    Possible,
    Unsupported(String),
}

/// Audio source selection types shared by audio, session and UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Target {
    /// Bundle id (macOS) or exe path (Windows).
    App(String),
    WholeSystem,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AudioApp {
    pub id: String,
    pub name: String,
    pub producing_audio: bool,
    pub pids: Vec<u32>,
}

/// What a [`SourceInfo`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    App,
    System,
}

/// One entry of the source picker (`source_list` = running apps that produce audio,
/// `source_list_installed` = installed apps for «Другое…»). The UI never asks for an id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SourceInfo {
    /// Stable id: bundle id (macOS) / exe path (Windows); `system` for the whole system.
    pub id: String,
    pub name: String,
    pub bundle_id: Option<String>,
    pub pid: Option<u32>,
    /// Small PNG icon, base64 (no `data:` prefix).
    pub icon_png_b64: Option<String>,
    pub producing_audio: bool,
    pub kind: SourceKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// 20 ms of 16 kHz mono f32 PCM (320 samples) from the audio worker.
/// Lives here so `lowk-stt` can consume it without depending on `lowk-audio`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Frame16k {
    pub stream: Stream,
    pub t_mono_ns: MonoNs,
    pub pcm: Vec<f32>,
}
