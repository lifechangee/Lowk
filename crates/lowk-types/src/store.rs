//! Vault DTOs (the on-disk format itself is owned by `lowk-store`).
use crate::{CallId, FolderRef, LangTag, QaId, SessionClock, SttEvent, Ulid};
use serde::{Deserialize, Serialize};
use specta::Type;

/// How the microphone is recorded during a call.
///
/// `Auto`: the microphone is open only while the selected call app itself holds an input (capture)
/// stream; otherwise it stays closed. Deserializes from the old `bool` setting too (`true` = on).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MicMode {
    #[default]
    On,
    Off,
    Auto,
}

impl<'de> Deserialize<'de> for MicMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = MicMode;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a bool or one of \"on\", \"off\", \"auto\"")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<MicMode, E> {
                Ok(if v { MicMode::On } else { MicMode::Off })
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<MicMode, E> {
                match v {
                    "on" | "true" => Ok(MicMode::On),
                    "off" | "false" => Ok(MicMode::Off),
                    "auto" => Ok(MicMode::Auto),
                    other => Err(E::unknown_variant(other, &["on", "off", "auto"])),
                }
            }
        }
        d.deserialize_any(V)
    }
}

/// Per-folder «record microphone» preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MicPreset {
    /// Follow the global setting (`audio.record_mic`).
    #[default]
    Inherit,
    On,
    Off,
    Auto,
}

impl MicPreset {
    /// The mode a new call in a folder with this preset starts in.
    pub fn resolve(self, global: MicMode) -> MicMode {
        match self {
            MicPreset::Inherit => global,
            MicPreset::On => MicMode::On,
            MicPreset::Off => MicMode::Off,
            MicPreset::Auto => MicMode::Auto,
        }
    }
}

/// Per-folder «re-transcribe after recording» preset (recommended on for lecture folders).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RetxPreset {
    /// Follow the global setting (`stt.retranscribe_after`).
    #[default]
    Inherit,
    On,
    Off,
}

impl RetxPreset {
    /// Whether a call that just ended in a folder with this preset is re-transcribed automatically.
    pub fn resolve(self, global: bool) -> bool {
        match self {
            RetxPreset::Inherit => global,
            RetxPreset::On => true,
            RetxPreset::Off => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FolderMeta {
    pub id: FolderRef,
    pub title: String,
    pub tags: Vec<String>,
    pub allow_cloud: bool,
    pub default_template: Option<String>,
    /// «Record microphone» preset for new calls in this folder.
    #[serde(default)]
    pub mic: MicPreset,
    /// «Re-transcribe after recording» preset for calls in this folder.
    #[serde(default)]
    pub retx: RetxPreset,
    /// Path relative to the vault root.
    pub path: String,
    pub call_count: u32,
}

/// A folder that sits in the Trash with everything it contained (restorable as a whole).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TrashedFolder {
    pub id: Ulid,
    pub title: String,
    /// RFC 3339 (UTC).
    pub deleted_at: String,
    pub calls: u32,
    pub docs: u32,
    /// The calls inside; the Trash list shows them under the folder, not on their own.
    pub call_ids: Vec<Ulid>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NewFolder {
    pub title: String,
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
    /// A state-change line in the transcript (e.g. «Микрофон выключен»), stamped with the call time.
    Marker {
        at_s: u32,
        text: String,
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

/// `vault_open` / `vault_info` payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct VaultInfo {
    /// Absolute path of the open vault.
    pub path: String,
}
