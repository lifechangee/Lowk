//! settings, locale, secrets, app lifecycle.
use crate::state::Shared;
use lowk_types::*;
use tauri::State;

#[tauri::command]
#[specta::specta]
pub fn settings_get(st: State<'_, Shared>) -> Settings {
    st.settings.get()
}

#[tauri::command(async)]
#[specta::specta]
pub fn settings_patch(st: State<'_, Shared>, patch: SettingsPatch) -> Result<Settings> {
    st.settings.patch(patch)
}

/// Raw system locale tag (`ru-RU`); the UI resolves `auto|ru|en` from it.
#[tauri::command]
#[specta::specta]
pub fn locale_get() -> String {
    lowk_platform::locale::system()
}

/// Providers that may hold a key. The value never leaves Rust after `secret_set`.
const PROVIDERS: [&str; 2] = ["openrouter", "stt-cloud"];

fn provider(p: &str) -> Result<&str> {
    PROVIDERS
        .iter()
        .copied()
        .find(|x| *x == p)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument).with_detail("unknown secret provider"))
}

#[tauri::command(async)]
#[specta::specta]
pub fn secret_set(st: State<'_, Shared>, provider_id: String, value: String) -> Result<()> {
    lowk_platform::secrets::set(provider(&provider_id)?, value.trim())?;
    if provider_id == "openrouter" {
        if let Some(svc) = st.m2.read().clone() {
            // a new key: pin the fastest provider, and let the cloud embedder (if chosen) start
            svc.spawn_provider_probe();
            st.spawn_ask_warmup(None);
            if st.settings.get().embedder.mode == EmbedderMode::Cloud {
                svc.apply_embedder();
            }
        }
    }
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn secret_status(provider_id: String) -> Result<bool> {
    Ok(lowk_platform::secrets::present(provider(&provider_id)?))
}

#[tauri::command(async)]
#[specta::specta]
pub fn secret_delete(provider_id: String) -> Result<()> {
    lowk_platform::secrets::delete(provider(&provider_id)?)
}

/// Quit the whole app (the overlay keeps it alive otherwise).
#[tauri::command]
#[specta::specta]
pub fn app_quit(st: State<'_, Shared>) {
    st.app.exit(0);
}

/// Relaunches Lowk (macOS applies Screen Recording / Input Monitoring / Accessibility grants
/// only to a freshly started process). From a bundle: a detached `open` starts the new instance
/// after this one has exited (the single-instance guard would otherwise hand off to the dying
/// one); in dev: plain `restart`.
#[tauri::command]
#[specta::specta]
pub fn app_relaunch(st: State<'_, Shared>) {
    #[cfg(target_os = "macos")]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bundle) = exe
            .ancestors()
            .nth(3)
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
        {
            let _ = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(format!(
                    "sleep 1; /usr/bin/open -n '{}'",
                    bundle.display().to_string().replace('\'', "")
                ))
                .spawn();
            st.app.exit(0);
            return;
        }
    }
    st.app.restart();
}

/// Opens (or focuses) the main window; used by the second-instance handler,
/// the Dock reopen event and the overlay.
#[tauri::command]
#[specta::specta]
pub fn main_open(st: State<'_, Shared>) {
    crate::windows::open_main(&st.app);
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UpdateStatus {
    /// False until the release signing keys exist (docs/DECISIONS-M0.md).
    pub enabled: bool,
    pub current: String,
    pub available: Option<String>,
}

/// Updater skeleton: the plugin is not wired until the minisign key exists.
#[tauri::command(async)]
#[specta::specta]
pub fn update_check(st: State<'_, Shared>) -> Result<UpdateStatus> {
    let _ = &st;
    Ok(UpdateStatus {
        enabled: false,
        current: env!("CARGO_PKG_VERSION").into(),
        available: None,
    })
}
