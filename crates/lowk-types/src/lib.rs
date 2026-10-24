//! `lowk-types`: the contract crate.
//!
//! Every type that crosses a crate or IPC boundary lives here, with `serde` and
//! `specta::Type` derives. Changes are architect-gated: this
//! crate is the seam that lets many engineers work in parallel.
//!
//! Rules: no business logic, no OS code, no `tauri` dependency. Small `Fake*` /
//! `Noop*` helpers live in [`fakes`] so dependents can merge independently.

pub mod bind;
pub mod docs;
pub mod error;
pub mod event;
pub mod fakes;
pub mod hw;
pub mod ids;
pub mod langpolicy;
pub mod llm;
pub mod overlay;
pub mod paths;
pub mod perm;
pub mod qa;
pub mod retx;
pub mod search;
pub mod session;
pub mod settings;
pub mod store;
pub mod stt;
pub mod text;
pub mod time;
pub mod traits;

pub use bind::*;
pub use docs::*;
pub use error::*;
pub use event::*;
pub use hw::*;
pub use ids::*;
pub use llm::*;
pub use overlay::*;
pub use perm::*;
pub use qa::*;
pub use retx::*;
pub use search::*;
pub use session::*;
pub use settings::*;
pub use store::*;
pub use stt::*;
pub use time::*;
pub use traits::*;

/// Crate-wide result alias; errors are [`AppError`] (code + params, never prose).
pub type Result<T, E = AppError> = std::result::Result<T, E>;

/// Every DTO that must appear in the generated TypeScript bindings is
/// registered here once so `cargo xtask bindings` cannot miss one.
pub fn register_types(types: &mut specta::Types) {
    types.register_mut::<AppEvent>();
    types.register_mut::<Settings>();
    types.register_mut::<SettingsPatch>();
    types.register_mut::<AppError>();
    types.register_mut::<ErrorCode>();
    types.register_mut::<SessionSnapshot>();
    types.register_mut::<StartOpts>();
    types.register_mut::<FolderMeta>();
    types.register_mut::<CallMeta>();
    types.register_mut::<NewFolder>();
    types.register_mut::<NewCall>();
    types.register_mut::<CallEntry>();
    types.register_mut::<Tone>();
    types.register_mut::<SelfTestReport>();
    types.register_mut::<BindReport>();
    types.register_mut::<BindSpec>();
    types.register_mut::<HwInfo>();
    types.register_mut::<HwReport>();
    types.register_mut::<HwProbe>();
    types.register_mut::<VaultInfo>();
    types.register_mut::<BindsState>();
    types.register_mut::<Recommendation>();
    types.register_mut::<QaRecord>();
    types.register_mut::<EngineDescriptor>();
    types.register_mut::<EngineInfo>();
    types.register_mut::<EngineProgress>();
    types.register_mut::<SttDiagnostics>();
    types.register_mut::<RetrievalQuery>();
    types.register_mut::<ContextChunk>();
    types.register_mut::<AskRequest>();
    types.register_mut::<AudioApp>();
    types.register_mut::<SourceInfo>();
    types.register_mut::<SourceKind>();
    types.register_mut::<DeviceInfo>();
    // M2 contracts (docs/DECISIONS-M2.md)
    types.register_mut::<Document>();
    types.register_mut::<DocKind>();
    types.register_mut::<IngestStatus>();
    types.register_mut::<IngestWarning>();
    types.register_mut::<NewDocument>();
    types.register_mut::<DupAction>();
    types.register_mut::<DuplicateHit>();
    types.register_mut::<DocPatch>();
    types.register_mut::<IngestProgress>();
    types.register_mut::<PinnedUsage>();
    types.register_mut::<SearchQuery>();
    types.register_mut::<SearchFilters>();
    types.register_mut::<SearchResults>();
    types.register_mut::<SearchHit>();
    types.register_mut::<SourceRef>();
    types.register_mut::<IndexStatus>();
    types.register_mut::<EmbedderInfo>();
    types.register_mut::<ContextPack>();
    types.register_mut::<ReindexScope>();
    types.register_mut::<IndexItem>();
    types.register_mut::<IndexFolder>();
    types.register_mut::<LlmEvent>();
    types.register_mut::<Summary>();
    types.register_mut::<SummaryEvent>();
    types.register_mut::<TagSuggestion>();
    types.register_mut::<LlmProviderStatus>();
}

/// Tauri event names (backend -> UI), one per [`AppEvent`] variant plus the
/// high-rate streams that bypass the bus. `lowk-app` and the UI both use these.
pub mod events {
    pub const STT_SEGMENT: &str = "stt://segment";
    pub const STT_ERROR: &str = "stt://error";
    /// Payload `{ "loading": bool }`: the local engine is loading its model.
    pub const STT_LOADING: &str = "stt://loading";
    /// Payload `{ "kind": "target_lost" | "target_restored" }`.
    pub const AUDIO_NOTICE: &str = "audio://notice";
    pub const QA_MY_ANSWER: &str = "qa://my-answer";
    pub const SESSION_STATE: &str = "session://state";
    /// A recording ended (any path); payload `{ "route": "home" }`. The app has just
    /// shown/focused the main window; the UI navigates it to the library home.
    pub const SESSION_ENDED: &str = "session://ended";
    pub const AUDIO_LEVEL: &str = "audio://level";
    pub const LLM_START: &str = "llm://start";
    pub const LLM_DELTA: &str = "llm://delta";
    pub const LLM_DONE: &str = "llm://done";
    pub const LLM_ERROR: &str = "llm://error";
    /// Payload `{ req_id, sources: SourceRef[], withheld }`.
    pub const LLM_SOURCES: &str = "llm://sources";
    /// Document ingest progress (`IngestProgress`), main window only.
    pub const DOC_PROGRESS: &str = "doc://progress";
    /// `IndexStatus`, main window only.
    pub const INDEX_STATUS: &str = "index://status";
    /// `SummaryEvent`, main window only.
    pub const SUMMARY_EVENT: &str = "summary://event";
    /// `RetxEvent`, main window only.
    pub const RETX_EVENT: &str = "retx://event";
    pub const OVERLAY_MODE: &str = "overlay://mode";
    pub const OVERLAY_TONE: &str = "overlay://tone";
    pub const STEALTH_STATUS: &str = "stealth://status";
    pub const QUEUE_CHANGED: &str = "queue://changed";
    pub const PERM_CHANGED: &str = "perm://changed";
    pub const BIND_EVENT: &str = "bind://event";
    pub const STORE_CHANGED: &str = "store://changed";
    pub const SETTINGS_CHANGED: &str = "settings://changed";
    pub const ENGINE_PROGRESS: &str = "engine://progress";
}

#[cfg(test)]
mod tests;
