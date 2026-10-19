//! Lowk store (WP4): vault of Markdown files, call/folder codecs, live journal
//! with crash recovery, trash, watcher, `settings.toml`, prompt templates.
//! No OS-specific code; builds on Linux.
pub mod call_codec;
pub mod fm;
pub mod folder_codec;
pub mod settings;
pub mod templates;
mod transfer;
mod util;
pub mod vault;
mod watch;
pub mod writer;

pub use settings::{FileSettingsStore, SETTINGS_FILE};
pub use templates::{summary_template, TemplateSet, TEMPLATE_SUMMARY};
pub use transfer::ImportReport;
pub use util::{atomic_write, sanitize_name};
pub use vault::{Call, FolderPatch, ItemKind, TrashItem, Vault, VaultInner};
pub use watch::VaultWatcher;
pub use writer::{CallWriter, Recoverable, WriterNotice};
