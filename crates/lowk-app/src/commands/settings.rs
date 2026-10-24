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

/// `explorer.exe` argument that selects `p` (or opens it when it is a folder). Windows quoting: the
/// whole `/select,"..."` is passed raw so Cyrillic / spaces survive; `"` cannot occur in a path.
#[cfg_attr(not(windows), allow(dead_code))]
fn explorer_arg(p: &std::path::Path) -> String {
    let s = p.display().to_string().replace('/', "\\").replace('"', "");
    if p.is_dir() {
        format!("\"{s}\"")
    } else {
        format!("/select,\"{s}\"")
    }
}

fn reveal(p: &std::path::Path) -> Result<()> {
    if !p.exists() {
        return Err(AppError::new(ErrorCode::NotFound).with_detail(p.display().to_string()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer.exe exits with 1 even on success: ignore the status.
        std::process::Command::new("explorer.exe")
            .raw_arg(explorer_arg(p))
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|e| AppError::new(ErrorCode::Io).with_detail(e.to_string()))?;
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = std::process::Command::new("/usr/bin/open");
        if p.is_dir() {
            c.arg(p);
        } else {
            c.arg("-R").arg(p);
        }
        c.spawn()
            .map_err(|e| AppError::new(ErrorCode::Io).with_detail(e.to_string()))?;
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let dir = if p.is_dir() { p } else { p.parent().unwrap_or(p) };
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
    Ok(())
}

/// Shows a vault / document path in the file manager (Explorer on Windows, Finder on macOS).
#[tauri::command(async)]
#[specta::specta]
pub fn path_reveal(path: String) -> Result<()> {
    reveal(&crate::state::resolve_vault_path(&path)?)
}

/// Opens the folder with `lowk.log` (and `lowk.prev.log`): what the owner sends us when something breaks.
#[tauri::command(async)]
#[specta::specta]
pub fn logs_reveal() -> Result<()> {
    let dir = crate::state::data_dir()?;
    let log = dir.join("lowk.log");
    reveal(if log.exists() { &log } else { &dir })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_arg_quotes_cyrillic_and_spaces() {
        let d = std::env::temp_dir();
        assert_eq!(
            explorer_arg(&d),
            format!("\"{}\"", d.display().to_string().replace('/', "\\"))
        );
        let f = std::path::Path::new("C:/Users/Иван Петров/AppData/Roaming/Lowk/lowk.log");
        assert_eq!(
            explorer_arg(f),
            "/select,\"C:\\Users\\Иван Петров\\AppData\\Roaming\\Lowk\\lowk.log\""
        );
    }

    #[test]
    fn reveal_of_a_missing_path_is_an_error() {
        assert!(reveal(std::path::Path::new("/definitely/not/here/lowk")).is_err());
    }
}
