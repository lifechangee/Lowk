//! The internal bus event. Each variant maps 1:1 to a Tauri event name in
//! [`crate::events`]. The bus carries low-rate events only.
use crate::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum AppEvent {
    Stt(SttEvent),
    Session(SessionState),
    Llm(LlmEvent),
    Overlay(OverlayEvent),
    Stealth(StealthStatus),
    Bind(BindEvent),
    Perm {
        kind: PermKind,
        state: PermState,
    },
    Queue(QueueState),
    Store(StoreChange),
    Level(AudioLevel),
    /// «Мой ответ» text grew or its link closed.
    MyAnswer(QaUpdate),
    /// An STT engine failed (isolated per stream).
    SttError(SttError),
    /// The local speech engine is loading its model (true) / is ready (false). Audio captured
    /// meanwhile is buffered and transcribed once it is.
    SttLoading(bool),
    /// Model download / verification progress.
    EngineProgress(EngineProgress),
}

impl AppEvent {
    /// Tauri event name for this variant (`Overlay` splits into mode/tone).
    pub fn name(&self) -> &'static str {
        use crate::events::*;
        match self {
            AppEvent::Stt(_) => STT_SEGMENT,
            AppEvent::Session(_) => SESSION_STATE,
            AppEvent::Llm(LlmEvent::Start { .. }) => LLM_START,
            AppEvent::Llm(LlmEvent::Delta { .. }) => LLM_DELTA,
            AppEvent::Llm(LlmEvent::Done { .. }) => LLM_DONE,
            AppEvent::Llm(LlmEvent::Error { .. }) => LLM_ERROR,
            AppEvent::Overlay(OverlayEvent::Mode(_)) => OVERLAY_MODE,
            AppEvent::Overlay(OverlayEvent::Tone(_)) => OVERLAY_TONE,
            AppEvent::Stealth(_) => STEALTH_STATUS,
            AppEvent::Bind(_) => BIND_EVENT,
            AppEvent::Perm { .. } => PERM_CHANGED,
            AppEvent::Queue(_) => QUEUE_CHANGED,
            AppEvent::Store(_) => STORE_CHANGED,
            AppEvent::Level(_) => AUDIO_LEVEL,
            AppEvent::MyAnswer(_) => QA_MY_ANSWER,
            AppEvent::SttError(_) => STT_ERROR,
            AppEvent::SttLoading(_) => STT_LOADING,
            AppEvent::EngineProgress(_) => ENGINE_PROGRESS,
        }
    }
}
