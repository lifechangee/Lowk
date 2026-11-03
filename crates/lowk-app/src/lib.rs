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
pub mod datadir;
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
pub mod updater;
mod visibility;
mod windows;

// The QA control channel must never exist in a shipped binary (see docs/testing.md).
#[cfg(all(feature = "qa", not(debug_assertions)))]
compile_error!("the `qa` feature is for debug QA builds only; it must not be enabled in release builds");

use tauri::{Manager, RunEvent};

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

/// stderr plus `<data root>/logs/lowk.log` (a bundle launched from Finder has no terminal; this is what
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
    let l = lowk_platform::dirs::layout()?;
    std::fs::create_dir_all(l.logs()).ok()?;
    let path = l.log_file();
    let big = std::fs::metadata(&path).is_ok_and(|m| m.len() > 2_000_000);
    if big {
        // keep the previous run's log next to the new one (a bug report needs both)
        let _ = std::fs::rename(&path, l.prev_log_file());
    }
    let mut o = std::fs::OpenOptions::new();
    o.create(true).append(true);
    o.open(path).ok()
}

/// Log timestamps in the user's local time with the UTC offset (`2026-10-25T23:25:15.898+03:00`), so a log
/// can be read against the wall clock of the person who sent it. (They were UTC `...Z`: three hours off
/// for the owner, and every report needed converting.)
struct LocalTime;

/// The offset measured once at start: the OS only reports it reliably while the process is single-threaded
/// (macOS / Linux); later reads fall back to this. Windows reports it at any time (and follows DST).
static START_OFFSET: std::sync::OnceLock<time::UtcOffset> = std::sync::OnceLock::new();

fn local_stamp(now: time::OffsetDateTime) -> String {
    use time::format_description::{parse_borrowed, BorrowedFormatItem};
    static FMT: std::sync::OnceLock<Vec<BorrowedFormatItem<'static>>> = std::sync::OnceLock::new();
    let fmt = FMT.get_or_init(|| {
        parse_borrowed::<2>("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3][offset_hour sign:mandatory]:[offset_minute]")
            .unwrap_or_default()
    });
    let offset = time::UtcOffset::local_offset_at(now)
        .ok()
        .or_else(|| START_OFFSET.get().copied())
        .unwrap_or(time::UtcOffset::UTC);
    now.to_offset(offset)
        .format(fmt)
        .unwrap_or_else(|_| now.to_string())
}

impl tracing_subscriber::fmt::time::FormatTime for LocalTime {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        w.write_str(&local_stamp(time::OffsetDateTime::now_utc()))
    }
}

/// Measures the local UTC offset while the process is still single-threaded (macOS / Linux refuse later) and
/// shares it with the crates that show times to the user (call titles).
fn capture_local_offset() {
    if let Ok(o) = time::UtcOffset::current_local_offset() {
        let _ = START_OFFSET.set(o);
        lowk_types::clock::set_local_offset_secs(o.whole_seconds());
    }
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    capture_local_offset();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_ansi(false)
        .with_timer(LocalTime)
        .with_writer(|| Tee)
        .try_init();
}

pub fn run() {
    // first, before datadir::startup spawns threads
    capture_local_offset();
    datadir::startup();
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
    let context = tauri::generate_context!();
    // The updater plugin exists only with a real signing key; without one the app never contacts the update server.
    let update_key_ok = updater::configured_pubkey(context.config()).is_some_and(|k| updater::key_usable(&k));
    if update_key_ok {
        tb = tb.plugin(tauri_plugin_updater::Builder::new().build());
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
            let ups = std::sync::Arc::new(updater::Updates::new(update_key_ok));
            app.manage(ups.clone());
            boot::setup(app)?;
            if let Some(st) = app.try_state::<state::Shared>() {
                updater::spawn(st.inner().clone(), ups);
            }
            Ok(())
        })
        .on_window_event(windows::on_window_event)
        .build(context)
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

#[cfg(test)]
mod log_time_tests {
    use super::*;

    #[test]
    fn stamp_has_milliseconds_and_an_explicit_offset() {
        let s =
            local_stamp(time::OffsetDateTime::from_unix_timestamp_nanos(1_792_959_915_898_000_000).unwrap());
        // 2026-10-25T20:25:15.898 UTC, written in this machine's zone with its offset
        let re_ok = s.len() == 29
            && s.as_bytes()[10] == b'T'
            && s.as_bytes()[19] == b'.'
            && (s.as_bytes()[23] == b'+' || s.as_bytes()[23] == b'-')
            && s.as_bytes()[26] == b':';
        assert!(re_ok, "{s}");
        assert!(!s.ends_with('Z'), "{s}");
        assert_eq!(&s[20..23], "898");
    }

    #[test]
    fn a_fixed_offset_shifts_the_clock_and_prints_it() {
        // the owner's case: UTC+3
        let now = time::OffsetDateTime::from_unix_timestamp_nanos(1_792_959_915_898_000_000).unwrap();
        let off = time::UtcOffset::from_hms(3, 0, 0).unwrap();
        let fmt = time::format_description::parse_borrowed::<2>(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3][offset_hour sign:mandatory]:[offset_minute]",
        )
        .unwrap();
        assert_eq!(
            now.to_offset(off).format(&fmt).unwrap(),
            "2026-10-25T23:25:15.898+03:00"
        );
    }
}
