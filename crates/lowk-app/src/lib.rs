//! Lowk Tauri shell: windows, capabilities, command/event glue, input wiring,
//! single instance, updater skeleton. The only crate that knows Tauri; the
//! business logic lives in the feature crates.
mod actions;
mod boot;
mod callstore;
mod capture;
mod combo;
mod commands;
mod emitter;
mod mainthread;
#[cfg(debug_assertions)]
mod probe;
mod state;
mod visibility;
mod windows;

use tauri::RunEvent;

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

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .try_init();
}

pub fn run() {
    init_tracing();
    let builder = specta_builder();
    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch focuses the running app.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::open_main(app);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(combo::on_shortcut)
                .build(),
        )
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            boot::setup(app)
        })
        .on_window_event(windows::on_window_event)
        .build(tauri::generate_context!())
        .expect("error while building Lowk");

    app.run(|handle, event| match event {
        // The hidden overlay keeps the app alive; only an explicit exit quits.
        RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => windows::open_main(handle),
        _ => {
            let _ = handle;
        }
    });
}
