//! Lowk Tauri shell. No business logic lives here.
mod commands;

use lowk_types::fakes::MemSettingsStore;
use std::sync::Arc;
use tauri::Manager;

/// The single source of truth for IPC: commands + exported DTO types.
pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    let mut types = specta::Types::default();
    lowk_types::register_types(&mut types);
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(commands::all_commands!())
        .types(&types)
        .dangerously_cast_bigints_to_number() // MonoNs/UttId stay < 2^53 (see DECISIONS-M0)
}

/// Writes `ui/src/lib/ipc/bindings.ts` (called by `cargo xtask bindings`).
pub fn export_bindings(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    specta_builder().export(specta_typescript::Typescript::default(), path)?;
    Ok(())
}

pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            // M0 placeholder: WP4 replaces this with the file-backed store.
            let store: commands::settings::Store = Arc::new(MemSettingsStore::default());
            app.manage(store);
            // No stealth logic yet (WP2). The overlay stays hidden; main is shown.
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Lowk");
}
