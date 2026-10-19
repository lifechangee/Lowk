/// Kept in sync with `src/commands/mod.rs` (append-only, one per line).
/// Each command gets an `allow-<command>` permission; capability files reference
/// only these (no `core:window:*` for either window).
const COMMANDS: &[&str] = &["settings_get", "settings_patch", "locale_get"];

fn main() {
    let manifest = tauri_build::AppManifest::new().commands(COMMANDS);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("tauri build failed");
}
