//! Command modules: one file per domain. This list is append-only, one line per
//! command; the same names are mirrored in `build.rs` (`COMMANDS`) so Tauri
//! generates `allow-<command>` permissions.
pub mod settings;

macro_rules! all_commands {
    () => {
        tauri_specta::collect_commands![
            crate::commands::settings::settings_get,
            crate::commands::settings::settings_patch,
            crate::commands::settings::locale_get,
        ]
    };
}
pub(crate) use all_commands;
