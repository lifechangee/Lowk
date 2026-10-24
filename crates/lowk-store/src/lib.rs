//! Lowk store: vault of Markdown files, call/folder codecs, live journal
//! with crash recovery, trash, watcher, `settings.toml`, prompt templates, documents (`Docs/`).
//! No OS-specific code; builds on Linux.
pub mod audio;
pub mod call_codec;
pub mod doc_codec;
mod docs;
pub mod fm;
pub mod folder_codec;
pub mod retx;
pub mod settings;
pub mod templates;
mod transfer;
mod util;
pub mod vault;
mod watch;
pub mod writer;

pub use doc_codec::estimate_tokens;
pub use docs::{VaultDocs, DOCS_SUBDIR};
pub use settings::{FileSettingsStore, SETTINGS_FILE};
pub use templates::TemplateSet;
pub use transfer::ImportReport;
pub use util::{atomic_write, now_in_offset_of, sanitize_name};
pub use vault::{Call, FolderPatch, ItemKind, TrashItem, Vault, VaultInner};
pub use watch::VaultWatcher;
pub use writer::{CallWriter, Recoverable, WriterNotice};
