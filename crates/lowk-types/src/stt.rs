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

/// Kind of an audio source in the picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    App,
    /// Everything except our own process tree.
    System,
}

/// One entry of the audio source picker (`source_list` / `source_list_installed`).
/// `id` is what goes into `Target::App(id)` (bundle id on macOS, exe path on
/// Windows) or the literal `"system"` for [`SourceKind::System`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SourceInfo {
    pub id: String,
    pub name: String,
    pub bundle_id: String,
    /// Main process id, 0 when the app is not running.
    pub pid: u32,
    /// Small PNG (<= 64 px) as base64 without the `data:` prefix.
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

/// One row of the engine picker (`engine_status`): the static descriptor plus
/// what this machine can do with it right now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct EngineInfo {
    pub descriptor: EngineDescriptor,
    pub fitness: Fitness,
    /// All assets present in app data and checksum-verified (always true for cloud).
    pub installed: bool,
    /// Picked by the hardware check for this machine.
    pub recommended: bool,
    /// Needs a key in the OS keychain (cloud engines).
    pub needs_key: bool,
    /// Link shown with the licence text before any download.
    pub license_url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum InstallPhase {
    Downloading,
    Verifying,
    Ready,
    Failed,
}

/// `engine://progress` payload (model downloads).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct EngineProgress {
    pub engine: String,
    /// Asset being fetched (file name).
    pub file: String,
    pub phase: InstallPhase,
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// Set for `Failed` (machine-readable code in `AppError` form is not needed here).
    pub message: Option<String>,
}

/// Latency percentiles in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Percentiles {
    pub samples: u32,
    pub p50_ms: u32,
    pub p95_ms: u32,
    pub max_ms: u32,
}

/// Per-stream STT health for the debug page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct StreamDiag {
    pub stream: Stream,
    /// Newest audio fed -> partial/final emitted by the engine (queue + compute),
    /// the part of word-to-screen the app controls. Algorithmic look-ahead of the
    /// model is on top (see `lookahead_ms` of `SttDiagnostics`).
    pub emit_lag: Percentiles,
    /// End of speech -> final emitted.
    pub final_lag: Percentiles,
    pub backlog_ms: u32,
    pub restarts: u32,
}

/// `stt_diagnostics` payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SttDiagnostics {
    pub engine: String,
    pub lookahead_ms: u32,
    pub streams: Vec<StreamDiag>,
    pub corrected: u32,
    pub correction_skipped: u32,
    pub correction_lag: Percentiles,
}
