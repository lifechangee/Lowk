//! LLM / retrieval contract.
use crate::{CallId, FolderId, QaId, ReqId, Stream};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Where a retrieval may look.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RetrievalScope {
    pub folder: Option<FolderId>,
    pub call: Option<CallId>,
    pub tags: Vec<String>,
    /// Include other folders (soft boost for the same folder still applies).
    pub all_folders: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RetrievalQuery {
    pub text: String,
    pub scope: RetrievalScope,
    /// Token budget for the packed result.
    pub budget_tokens: u32,
    /// Chunk ids already sent in this call (not resent).
    pub exclude: Vec<String>,
    /// Withhold chunks from folders with `allow_cloud = false`.
    pub cloud_provider: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChunkKind {
    Speech,
    Doc,
    Qa,
    Image,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ContextChunk {
    pub id: String,
    pub kind: ChunkKind,
    pub text: String,
    /// Display source: file title.
    pub source: String,
    pub heading_path: Vec<String>,
    pub page: Option<u32>,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AskRequest {
    pub template_id: String,
    pub typed_text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LlmUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cached_tokens: u32,
}

/// `llm://start|delta|done|error`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmEvent {
    Start {
        req_id: ReqId,
        qa_id: QaId,
    },
    Delta {
        req_id: ReqId,
        text: String,
    },
    Done {
        req_id: ReqId,
        truncated: bool,
        usage: Option<LlmUsage>,
    },
    Error {
        req_id: ReqId,
        error: crate::AppError,
    },
}

/// A line of the live transcript as kept by `TranscriptBuf`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TranscriptLine {
    pub seq: u64,
    pub stream: Stream,
    pub offset_s: u32,
    pub text: String,
    pub is_final: bool,
}
