//! Identifiers.
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;
use std::str::FromStr;

/// ULID, serialized as its 26-char string. Identity of calls, folders, docs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct Ulid(#[specta(type = String)] ulid::Ulid);

impl Ulid {
    pub fn new() -> Self {
        Self(ulid::Ulid::generate())
    }
    pub fn from_parts(timestamp_ms: u64, random: u128) -> Self {
        Self(ulid::Ulid::from_parts(timestamp_ms, random))
    }
    pub fn timestamp_ms(&self) -> u64 {
        self.0.timestamp_ms()
    }
}
impl Default for Ulid {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
impl fmt::Debug for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ulid({})", self.0)
    }
}
impl FromStr for Ulid {
    type Err = ulid::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ulid::Ulid::from_string(s).map(Self)
    }
}

pub type CallId = Ulid;
pub type FolderId = Ulid;
pub type DocId = Ulid;
pub type ReqId = Ulid;
pub type QaId = Ulid;

/// Utterance id, unique per stream within one call.
pub type UttId = u64;

/// Folder reference: a user folder (ULID) or a reserved system folder.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum FolderRef {
    User(Ulid),
    /// `sys:unsorted` — calls recorded without a chosen folder.
    Unsorted,
    /// `sys:trash` — app-level, restorable trash.
    Trash,
}

/// Reserved system folder ids and on-disk names (UI shows localized names).
pub const SYS_UNSORTED_ID: &str = "sys:unsorted";
pub const SYS_TRASH_ID: &str = "sys:trash";
pub const UNSORTED_DIR: &str = "Unsorted";
pub const TRASH_DIR: &str = ".lowk/trash";
