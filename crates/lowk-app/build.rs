/// Kept in sync with `src/commands/mod.rs` (append-only, one per line).
/// Each command gets an `allow-<command>` permission; capability files reference
/// only these (no `core:window:*` for either window).
const COMMANDS: &[&str] = &[
    "settings_get",
    "settings_patch",
    "locale_get",
    "secret_set",
    "secret_status",
    "secret_delete",
    "app_quit",
    "main_open",
    "update_check",
    "vault_open",
    "vault_info",
    "folder_list",
    "folder_get",
    "folder_create",
    "folder_update",
    "folder_delete",
    "call_list",
    "call_summarized",
    "call_get",
    "call_get_md",
    "call_save_md",
    "call_move",
    "call_delete",
    "call_restore",
    "call_rename",
    "call_set_tags",
    "trash_empty",
    "export_vault",
    "import_vault",
    "stealth_status",
    "stealth_selftest",
    "overlay_set_mode",
    "overlay_show",
    "overlay_hide",
    "overlay_escape",
    "overlay_drag_start",
    "overlay_drag_to",
    "overlay_drag_end",
    "overlay_input_submit",
    "overlay_tone",
    "binds_get",
    "binds_set",
    "binds_record_start",
    "binds_record_stop",
    "perm_status",
    "perm_request",
    "perm_open_settings",
    "hw_probe",
    "hw_bench",
    "session_start",
    "session_stop",
    "session_pause",
    "session_resume",
    "session_state",
    "session_snapshot",
    "source_list",
    "engine_list",
    "engine_install",
    "demo_start",
    "llm_ask",
    "llm_cancel",
    "llm_set_template",
    "capture_screenshot",
    "queue_clear",
];

fn main() {
    // Swift runtime (screencapturekit links through @rpath); dependency link-args do not propagate.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    let manifest = tauri_build::AppManifest::new().commands(COMMANDS);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("tauri build failed");
}
