//! Vault DTOs (the on-disk format itself is owned by `lowk-store`).
use crate::{CallId, FolderRef, LangTag, QaId, SessionClock, SttEvent, Ulid};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FolderMeta {
    pub id: FolderRef,
    pub title: String,
    pub tags: Vec<String>,
    pub allow_cloud: bool,
    pub default_template: Option<String>,
    /// Path relative to the vault root.
    pub path: String,
    pub call_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NewFolder {
    pub title: String,
    pub tags: Vec<String>,
    pub allow_cloud: bool,
    pub instructions: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CallSources {
    pub me: Option<String>,
    pub system: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CallMeta {
    pub id: CallId,
    pub title: String,
    pub folder: FolderRef,
    /// RFC 3339 with offset.
    pub started: String,
    pub ended: Option<String>,
    pub duration_s: Option<u64>,
    pub languages: Vec<LangTag>,
    pub tags: Vec<String>,
    pub sources: CallSources,
    pub stt_engine: Option<String>,
    pub path: String,
    pub deleted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NewCall {
    pub title: String,
    pub folder: FolderRef,
    pub started: String,
    pub sources: CallSources,
    pub stt_engine: Option<String>,
    pub languages: Vec<LangTag>,
    /// Labels written to the `speakers` table, chosen from the UI language.
    pub speaker_me: String,
    pub speaker_system: String,
    /// Session clock (monotonic <-> wall) so `CallEntry::Final` times map to
    /// offsets; `None` = the writer starts its own clock now.
    #[serde(default)]
    pub clock: Option<SessionClock>,
}

/// What `CallWriter::push` accepts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CallEntry {
    Final {
        event: SttEvent,
    },
    Qa {
        id: QaId,
        at_s: u32,
        template: String,
        model: String,
        range: String,
        question: String,
        answer: String,
    },
    Image {
        at_s: u32,
        rel_path: String,
        source: String,
        caption: String,
    },
    MyAnswer {
        qa_id: QaId,
        at_s: u32,
        text: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StoreChangeKind {
    Created,
    Updated,
    Moved,
    Trashed,
    Restored,
    Removed,
}

/// `store://changed` payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StoreChange {
    pub kind: StoreChangeKind,
    pub id: Ulid,
    pub is_folder: bool,
}
