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
    "app_relaunch",
    "main_open",
    "update_check",
    "path_reveal",
    "logs_reveal",
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
    "pill_drag_start",
    "pill_drag_to",
    "pill_drag_end",
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
    "source_list_installed",
    "engine_list",
    "engine_install",
    "engine_status",
    "engine_install_cancel",
    "stt_diagnostics",
    "demo_start",
    "llm_ask",
    "llm_cancel",
    "llm_set_template",
    "capture_screenshot",
    "queue_clear",
    "area_confirm",
    "area_cancel",
    "doc_list",
    "doc_get",
    "doc_import",
    "doc_find_duplicates",
    "doc_get_md",
    "doc_save_md",
    "doc_patch",
    "doc_delete",
    "doc_restore",
    "doc_retry",
    "doc_cancel",
    "docs_pinned_usage",
    "search_query",
    "index_status",
    "index_rebuild",
    "embedder_info",
    "embedder_install",
    "embedder_install_cancel",
    "source_open",
    "settings_open",
    "llm_summarize",
    "llm_summarize_cancel",
    "retx_status",
    "retx_start",
    "retx_cancel",
    "retx_restore",
    "session_set_mic",
    "session_set_mic_mode",
    "trash_delete_forever",
    "llm_suggest_tags",
    "llm_provider_status",
    "llm_key_test",
    "llm_detect_local",
    "folder_export",
    "folder_import",
    "app_about",
    "licenses_text",
    "diagnostics_save",
];

/// Short commit hash of the checkout, `unknown` without git (source tarball, CI without history).
fn git_commit() -> String {
    println!("cargo:rerun-if-env-changed=LOWK_GIT_COMMIT");
    if let Some(c) = std::env::var("LOWK_GIT_COMMIT")
        .ok()
        .filter(|c| !c.trim().is_empty())
    {
        return c.trim().chars().take(12).collect();
    }
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    };
    // rebuild when HEAD moves (works for worktrees: the path comes from git itself)
    for p in ["HEAD", "logs/HEAD"] {
        if let Some(path) = git(&["rev-parse", "--git-path", p]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".into())
}

/// UTC date `YYYY-MM-DD` (civil-from-days, no date crate). `SOURCE_DATE_EPOCH` makes it reproducible.
fn build_date() -> String {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });
    let Some(secs) = secs else { return "unknown".into() };
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn main() {
    println!("cargo:rustc-env=LOWK_GIT_COMMIT={}", git_commit());
    println!("cargo:rustc-env=LOWK_BUILD_DATE={}", build_date());
    // The Swift runtime rpath (screencapturekit, cidre) comes from .cargo/config.toml.
    let manifest = tauri_build::AppManifest::new().commands(COMMANDS);
    // Windows: the default Tauri manifest (common controls v6) plus `longPathAware` (vault/model paths > 260 chars).
    let windows =
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-app-manifest.xml"));
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(manifest)
            .windows_attributes(windows),
    )
    .expect("tauri build failed");
}
