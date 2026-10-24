//! «Перераспознать запись»: DTOs of the offline re-transcription pass (events, status, revisions).
use crate::{AppError, CallId};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Which engine a pass uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RetxBackend {
    /// Whisper large-v3-turbo in the local worker (the default).
    Local,
    /// Deepgram's pre-recorded API (explicit opt-in, never for local-only folders).
    Cloud,
}

/// `retx://event` payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RetxEvent {
    Start {
        call: CallId,
        total_s: u32,
        backend: RetxBackend,
    },
    Progress {
        call: CallId,
        done_s: u32,
        total_s: u32,
    },
    /// A recording started: the pass waits and resumes when it ends.
    Paused {
        call: CallId,
    },
    Resumed {
        call: CallId,
    },
    /// The new revision is written (`rev` = its number, `replaced` = utterances changed).
    Done {
        call: CallId,
        rev: u32,
        chunks: u32,
    },
    Cancelled {
        call: CallId,
    },
    Error {
        call: CallId,
        error: AppError,
    },
}

/// `retx_status`: what a freshly opened call page needs to know.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct RetxStatus {
    /// Saved audio exists for the call (the action is offered).
    pub has_audio: bool,
    /// A pass is running / waiting for this call.
    pub running: bool,
    pub paused: bool,
    pub done_s: u32,
    pub total_s: u32,
    /// Number of the transcript revision in the `.md` (1 = the live transcript, never re-transcribed).
    pub rev: u32,
    /// Older revisions kept on disk (restorable).
    pub revisions: Vec<RetxRevision>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RetxRevision {
    pub rev: u32,
    /// `live` (the real-time transcript) or `retx` (a previous re-transcription).
    pub kind: String,
    /// RFC 3339 time the revision was set aside.
    pub at: String,
}
