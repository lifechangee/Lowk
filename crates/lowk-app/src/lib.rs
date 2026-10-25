//! Lowk Tauri shell: windows, capabilities, command/event glue, input wiring,
//! single instance, updater skeleton. The only crate that knows Tauri; the
//! business logic lives in the feature crates.
mod actions;
mod area;
pub mod audio_selfcheck;
mod boot;
mod callstore;
mod capture;
mod combo;
mod commands;
mod diagnostics;
mod emitter;
mod engines;
pub mod m2;
mod mainthread;
#[cfg(any(debug_assertions, feature = "qa"))]
mod probe;
#[cfg(feature = "qa")]
mod qa;
mod sigterm;
pub mod soak;
mod state;
mod visibility;
mod windows;

// The QA control channel must never exist in a shipped binary (see docs/testing.md).
#[cfg(all(feature = "qa", not(debug_assertions)))]
compile_error!("the `qa` feature is for debug QA builds only; it must not be enabled in release builds");

use tauri::RunEvent;

/// The single source of truth for IPC: commands + exported DTO types.
pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    let mut types = specta::Types::default();
    lowk_types::register_types(&mut types);
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(commands::all_commands!())
        .types(&types)
        .dangerously_cast_bigints_to_number() // MonoNs/UttId stay < 2^53
}

/// The bundled third-party licence list (Settings -> About).
pub fn about_licenses() -> &'static str {
    commands::settings::THIRD_PARTY
}

/// Writes `ui/src/lib/ipc/bindings.ts` (called by `cargo xtask bindings`).
pub fn export_bindings(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    specta_builder().export(specta_typescript::Typescript::default(), path)?;
    Ok(())
}

/// stderr plus `<data dir>/lowk.log` (a bundle launched from Finder has no terminal; this is what
/// «why does X not work in the real app» is answered from). The file restarts past 2 MB.
struct Tee;
static LOG: std::sync::OnceLock<Option<std::sync::Mutex<std::fs::File>>> = std::sync::OnceLock::new();
fn log() -> Option<std::sync::MutexGuard<'static, std::fs::File>> {
    LOG.get_or_init(|| log_file().map(std::sync::Mutex::new))
        .as_ref()
        .and_then(|m| m.lock().ok())
}
impl std::io::Write for Tee {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if let Some(mut f) = log() {
            let _ = f.write_all(b);
        }
        std::io::stderr().write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(mut f) = log() {
            let _ = f.flush();
        }
        std::io::stderr().flush()
    }
}

/// Appends text to `lowk.log` and flushes (panic hook: the process may be gone a moment later).
pub(crate) fn log_raw(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(mut f) = log() {
        f.write_all(text.as_bytes())?;
        f.flush()?;
    }
    Ok(())
}

fn log_file() -> Option<std::fs::File> {
    let dir = state::data_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("lowk.log");
    let big = std::fs::metadata(&path).is_ok_and(|m| m.len() > 2_000_000);
    if big {
        // keep the previous run's log next to the new one (a bug report needs both)
        let _ = std::fs::rename(&path, dir.join("lowk.prev.log"));
    }
    let mut o = std::fs::OpenOptions::new();
    o.create(true).append(true);
    o.open(path).ok()
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_ansi(false)
        .with_writer(|| Tee)
        .try_init();
}

pub fn run() {
    init_tracing();
    diagnostics::install_panic_hook();
    let builder = specta_builder();
    let mut tb = tauri::Builder::default();
    // QA builds may run next to a developer's own instance (`LOWK_QA_MULTI=1`): no single-instance hand-off.
    #[cfg(feature = "qa")]
    let single = std::env::var_os("LOWK_QA_MULTI").is_none();
    #[cfg(not(feature = "qa"))]
    let single = true;
    if single {
        // Must be the first plugin: a second launch focuses the running app.
        tb = tb.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::open_main(app);
        }));
    }
    let app = tb
        .plugin(tauri_plugin_dialog::init())
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
