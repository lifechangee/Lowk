//! Errors: a code plus params. Rust never ships user-facing prose; the UI
//! translates `ErrorCode` (+ params). Display strings are English for logs.
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    #[error("unknown error")]
    Unknown,
    #[error("invalid argument")]
    InvalidArgument,
    #[error("not found")]
    NotFound,
    #[error("already exists")]
    AlreadyExists,
    #[error("not supported on this platform")]
    Unsupported,
    #[error("not implemented yet")]
    NotImplemented,
    #[error("i/o error")]
    Io,
    #[error("permission denied by the operating system")]
    PermissionDenied,
    #[error("microphone permission missing")]
    PermMicrophone,
    #[error("system audio permission missing")]
    PermSystemAudio,
    #[error("screen recording permission missing")]
    PermScreenRecording,
    #[error("input monitoring permission missing")]
    PermInputMonitoring,
    #[error("accessibility permission missing")]
    PermAccessibility,
    #[error("vault is not open")]
    VaultNotOpen,
    #[error("vault path is invalid")]
    VaultInvalid,
    #[error("file format error")]
    FormatInvalid,
    #[error("settings are invalid")]
    SettingsInvalid,
    #[error("keychain error")]
    SecretStore,
    #[error("audio device error")]
    AudioDevice,
    #[error("audio target lost")]
    AudioTargetLost,
    #[error("speech engine failed")]
    SttEngine,
    #[error("speech engine backlog")]
    SttBacklog,
    #[error("speech engine is not installed or has no key")]
    EngineNotReady,
    #[error("engine asset download failed")]
    DownloadFailed,
    #[error("network error")]
    Network,
    #[error("llm provider error")]
    LlmProvider,
    #[error("llm request cancelled")]
    LlmCancelled,
    #[error("api key missing")]
    ApiKeyMissing,
    #[error("model has no vision support")]
    NoVision,
    #[error("stealth could not be verified")]
    StealthUnavailable,
    #[error("a session is already running")]
    SessionBusy,
    #[error("no session is running")]
    SessionIdle,
    #[error("bind conflict")]
    BindConflict,
    #[error("ingest failed")]
    IngestFailed,
    #[error("search index error")]
    Index,
}

/// Serializable error crossing crate and IPC boundaries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AppError {
    pub code: ErrorCode,
    /// Interpolation params for the UI translation (`{name}` placeholders).
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// English diagnostic detail for logs; never shown as-is, never secrets.
    #[serde(default)]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
            detail: None,
        }
    }
    pub fn with_detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }
    pub fn with_param(mut self, k: impl Into<String>, v: impl ToString) -> Self {
        self.params.insert(k.into(), v.to_string());
        self
    }
    pub fn not_implemented() -> Self {
        Self::new(ErrorCode::NotImplemented)
    }
}
impl From<ErrorCode> for AppError {
    fn from(c: ErrorCode) -> Self {
        Self::new(c)
    }
}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::new(ErrorCode::Io).with_detail(e.to_string())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)?;
        if let Some(d) = &self.detail {
            write!(f, ": {d}")?;
        }
        Ok(())
    }
}
impl std::error::Error for AppError {}
