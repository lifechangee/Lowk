//! Command modules: one file per domain. This list is append-only, one line per
//! command; the same names are mirrored in `build.rs` (`COMMANDS`) so Tauri
//! generates `allow-<command>` permissions.
pub mod input;
pub mod platform;
pub mod session;
pub mod settings;
pub mod stealth;
pub mod vault;

macro_rules! all_commands {
    () => {
        tauri_specta::collect_commands![
            crate::commands::settings::settings_get,
            crate::commands::settings::settings_patch,
            crate::commands::settings::locale_get,
            crate::commands::settings::secret_set,
            crate::commands::settings::secret_status,
            crate::commands::settings::secret_delete,
            crate::commands::settings::app_quit,
            crate::commands::settings::main_open,
            crate::commands::settings::update_check,
            crate::commands::vault::vault_open,
            crate::commands::vault::vault_info,
            crate::commands::vault::folder_list,
            crate::commands::vault::folder_get,
            crate::commands::vault::folder_create,
            crate::commands::vault::folder_update,
            crate::commands::vault::folder_delete,
            crate::commands::vault::call_list,
            crate::commands::vault::call_summarized,
            crate::commands::vault::call_get,
            crate::commands::vault::call_get_md,
            crate::commands::vault::call_save_md,
            crate::commands::vault::call_move,
            crate::commands::vault::call_delete,
            crate::commands::vault::call_restore,
            crate::commands::vault::call_rename,
            crate::commands::vault::call_set_tags,
            crate::commands::vault::trash_empty,
            crate::commands::vault::export_vault,
            crate::commands::vault::import_vault,
            crate::commands::stealth::stealth_status,
            crate::commands::stealth::stealth_selftest,
            crate::commands::stealth::overlay_set_mode,
            crate::commands::stealth::overlay_show,
            crate::commands::stealth::overlay_hide,
            crate::commands::stealth::overlay_escape,
            crate::commands::stealth::overlay_drag_start,
            crate::commands::stealth::overlay_drag_to,
            crate::commands::stealth::overlay_drag_end,
            crate::commands::stealth::overlay_input_submit,
            crate::commands::stealth::overlay_tone,
            crate::commands::input::binds_get,
            crate::commands::input::binds_set,
            crate::commands::input::binds_record_start,
            crate::commands::input::binds_record_stop,
            crate::commands::platform::perm_status,
            crate::commands::platform::perm_request,
            crate::commands::platform::perm_open_settings,
            crate::commands::platform::hw_probe,
            crate::commands::platform::hw_bench,
            crate::commands::session::session_start,
            crate::commands::session::session_stop,
            crate::commands::session::session_pause,
            crate::commands::session::session_resume,
            crate::commands::session::session_state,
            crate::commands::session::session_snapshot,
            crate::commands::session::source_list,
            crate::commands::session::engine_list,
            crate::commands::session::engine_install,
            crate::commands::session::demo_start,
            crate::commands::session::llm_ask,
            crate::commands::session::llm_cancel,
            crate::commands::session::llm_set_template,
            crate::commands::session::capture_screenshot,
            crate::commands::session::queue_clear,
        ]
    };
}
pub(crate) use all_commands;
