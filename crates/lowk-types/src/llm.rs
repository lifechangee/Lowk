//! LLM / retrieval contract.
use crate::{AppError, CallId, FolderId, LangTag, QaId, ReqId, SourceRef, Stream};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Where a retrieval may look.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RetrievalScope {
    pub folder: Option<FolderId>,
    /// The call being recorded: its own transcript is never retrieved (it is sent as the
    /// live delta). Other calls of the folder are.
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
    /// Token cap for pinned documents (`Settings.docs.pinned_budget_tokens`).
    #[serde(default)]
    pub pinned_budget_tokens: u32,
    /// Include folder instructions / pinned documents in `retrieve_pack`.
    #[serde(default)]
    pub include_folder_context: bool,
    /// Search previous calls of the folder too (speech chunks), not only documents.
    #[serde(default)]
    pub include_previous_calls: bool,
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
    /// Clickable origin shown under the answer.
    pub source_ref: SourceRef,
    pub pinned: bool,
    /// Rough token count (budget packing).
    pub tokens: u32,
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
    /// Sent once after `Start`, before the first delta: what the answer was grounded on
    /// (deduplicated; instructions, pinned documents, documents, previous calls).
    Sources {
        req_id: ReqId,
        sources: Vec<SourceRef>,
        /// Chunks withheld by a folder's `allow_cloud = false`.
        withheld: u32,
    },
    /// Sent once before the first delta: which model answers and how many screenshots go with the Ask.
    Meta {
        req_id: ReqId,
        model: String,
        images: u32,
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

// --- summary, tags, provider (M2; ready for the M3 Ask flow) ---------------

/// A task from the call: who does what.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SummaryTask {
    pub who: Option<String>,
    pub what: String,
}

/// A question put to the user and how they answered («Мой ответ»).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SummaryQuestion {
    pub question: String,
    pub my_answer: Option<String>,
}

/// Post-call summary: 3-5 key points, decisions, tasks, questions to me and my answers.
/// `md` is what is written into `## Summary` of the call file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Summary {
    pub call: CallId,
    pub key_points: Vec<String>,
    pub decisions: Vec<String>,
    pub tasks: Vec<SummaryTask>,
    pub questions: Vec<SummaryQuestion>,
    pub md: String,
    pub model: String,
    /// RFC 3339.
    pub generated_at: String,
}

/// One Q&A chain as the summary sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct QaDigest {
    pub question: String,
    pub hint: String,
    pub my_answer: Option<String>,
}

/// Input of `LlmService::summarize`; `lowk-llm` never reads the vault.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SummarizeInput {
    pub call: CallId,
    pub title: String,
    /// Transcript as `[HH:MM:SS] **Speaker:** text` lines (map-reduced when too long).
    pub transcript: String,
    /// Q&A callouts: question, AI hint, my answer.
    pub qa: Vec<QaDigest>,
    pub languages: Vec<LangTag>,
    /// Language of the summary; `None` = the dominant call language.
    pub output_language: Option<LangTag>,
    pub folder_instructions: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TagSuggestion {
    pub tag: String,
    pub confidence: f32,
    /// Already used somewhere in the vault (preferred over inventing new tags).
    pub existing: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TagInput {
    pub call: CallId,
    pub title: String,
    pub summary: Option<String>,
    /// Head of the transcript when there is no summary yet.
    pub transcript_excerpt: String,
    /// Tags already used in the vault (the model picks from them first).
    pub vocabulary: Vec<String>,
    pub current_tags: Vec<String>,
    pub max: u32,
}

/// `summary://event` payload (separate from `llm://*`, which belongs to the overlay).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SummaryEvent {
    Start {
        call: CallId,
    },
    /// A long call is summarised in parts: request `step` of an estimated `total` is starting.
    Progress {
        call: CallId,
        step: u32,
        total: u32,
    },
    /// Streamed Markdown of the summary body.
    Delta {
        call: CallId,
        text: String,
    },
    Done {
        call: CallId,
        summary: Summary,
    },
    Error {
        call: CallId,
        error: AppError,
    },
}

/// `llm_provider_status`: what Settings shows next to the key field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LlmProviderStatus {
    pub provider: crate::LlmProvider,
    pub model: String,
    /// OpenRouter upstream provider the model is pinned to (after the latency probe).
    pub pinned_provider: Option<String>,
    pub key_present: bool,
}

/// Implemented by `lowk-llm` (cloud: OpenRouter). Blocking; cancel by `should_continue`
/// returning `false`. Errors: `ApiKeyMissing`, `Network`, `LlmProvider`, `LlmCancelled`.
pub trait LlmService: Send + Sync {
    fn summarize(
        &self,
        input: &SummarizeInput,
        on_delta: &mut dyn FnMut(&str),
        should_continue: &dyn Fn() -> bool,
    ) -> crate::Result<Summary>;
    fn suggest_tags(&self, input: &TagInput) -> crate::Result<Vec<TagSuggestion>>;
}

// --- screenshots going with an Ask ----------------------------------------

/// A screenshot waiting in the queue (PNG as captured). Not an IPC type: pixels never cross the
/// Tauri boundary except as thumbnails.
#[derive(Clone, Debug)]
pub struct QueuedImage {
    /// Unique per process; the queue removes exactly the ids that were sent.
    pub id: u64,
    /// Monitor index within one press, `primary` as reported by the platform.
    pub index: u32,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
    pub png: std::sync::Arc<Vec<u8>>,
}

/// A screenshot as it was actually sent (downscaled and encoded); the call stores these.
#[derive(Clone, Debug)]
pub struct SentImage {
    pub id: u64,
    /// `jpg` or `png`.
    pub ext: &'static str,
    pub bytes: std::sync::Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}
