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
    let next = st.settings.patch(patch)?;
    apply_network(&next)?;
    Ok(next)
}

/// Makes the manual proxy (Settings -> Network) effective for every outbound client at once.
/// A value that is not an http / socks5 proxy URL is refused instead of silently ignored.
pub fn apply_network(s: &Settings) -> Result<()> {
    if lowk_platform::proxy::set_manual(&s.network.proxy) {
        Ok(())
    } else {
        Err(AppError::new(ErrorCode::InvalidArgument)
            .with_detail("proxy must look like http://<host>:<port> or socks5://<host>:<port>"))
    }
}

/// Settings -> Network: the proxy found in the environment / OS settings (read-only).
#[tauri::command]
#[specta::specta]
pub fn network_status() -> NetworkStatus {
    use lowk_platform::proxy::Source;
    match lowk_platform::proxy::describe_system() {
        Some((url, src)) => NetworkStatus {
            system_proxy: Some(url),
            system_source: match src {
                Source::Env => Some(ProxySource::Env),
                Source::Windows => Some(ProxySource::Windows),
                Source::Macos => Some(ProxySource::Macos),
                Source::Manual => None,
            },
        },
        None => NetworkStatus {
            system_proxy: None,
            system_source: None,
        },
    }
}

/// «Проверить подключение»: GET openrouter.ai/api/v1/models through `proxy` (empty = the effective
/// system / saved proxy) and report status and latency. Never sends the API key.
#[tauri::command(async)]
#[specta::specta]
pub async fn network_check(proxy: String) -> Result<NetworkCheck> {
    tauri::async_runtime::spawn_blocking(move || lowk_llm::net_check(&proxy, lowk_llm::NET_CHECK_URL))
        .await
        .map_err(|e| AppError::new(ErrorCode::Network).with_detail(e.to_string()))
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
    relaunch(&st.app);
}

/// See [`app_relaunch`]; also used by the updater once an update is installed.
pub fn relaunch(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bundle) = exe
            .ancestors()
            .nth(3)
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
        {
            let _ = lowk_platform::process::command("/bin/sh")
                .arg("-c")
                .arg(format!(
                    "sleep 1; /usr/bin/open -n '{}'",
                    bundle.display().to_string().replace('\'', "")
                ))
                .spawn();
            app.exit(0);
            return;
        }
    }
    app.restart();
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
        lowk_platform::process::command("explorer.exe")
            .raw_arg(explorer_arg(p))
            .spawn()
            .map_err(|e| AppError::new(ErrorCode::Io).with_detail(e.to_string()))?;
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = lowk_platform::process::command("/usr/bin/open");
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
        let _ = lowk_platform::process::command("xdg-open").arg(dir).spawn();
    }
    Ok(())
}

/// Shows a vault / document path in the file manager (Explorer on Windows, Finder on macOS).
#[tauri::command(async)]
#[specta::specta]
pub fn path_reveal(path: String) -> Result<()> {
    reveal(&crate::state::resolve_vault_path(&path)?)
}

/// Opens the folder with `lowk.log` (and `lowk.prev.log`): what a user sends us when something breaks.
#[tauri::command(async)]
#[specta::specta]
pub fn logs_reveal() -> Result<()> {
    let l = lowk_platform::dirs::layout().ok_or_else(|| AppError::new(ErrorCode::Io))?;
    let log = l.log_file();
    reveal(if log.exists() { &log } else { &l.data_root })
}

/// Settings -> Storage: where models, engines and logs live, and whether a move is running.
#[tauri::command(async)]
#[specta::specta]
pub fn data_dir_info() -> Result<crate::datadir::DataDirInfo> {
    crate::datadir::info()
}

/// Moves models and engines to `path` (`null` = the default folder) in the background; progress is in `data_dir_info`.
#[tauri::command(async)]
#[specta::specta]
pub fn data_dir_move(path: Option<String>) -> Result<()> {
    crate::datadir::start_move(path)
}

/// Static facts for Settings -> About. `commit` and `build_date` are embedded by build.rs
/// (`unknown` when the build had no git or clock).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AboutInfo {
    pub version: String,
    pub commit: String,
    pub build_date: String,
    pub license: String,
    pub source_url: String,
}

pub const SOURCE_URL: &str = "https://github.com/lowk-app/lowk";

pub fn about_info() -> AboutInfo {
    AboutInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        commit: env!("LOWK_GIT_COMMIT").into(),
        build_date: env!("LOWK_BUILD_DATE").into(),
        license: "AGPL-3.0-or-later".into(),
        source_url: SOURCE_URL.into(),
    }
}

#[tauri::command]
#[specta::specta]
pub fn app_about() -> AboutInfo {
    about_info()
}

/// Third-party licences (`THIRD_PARTY.md`, generated by `just licenses`), compiled into the app
/// and also shipped as a bundle resource.
pub const THIRD_PARTY: &str = include_str!("../../../../THIRD_PARTY.md");

#[tauri::command]
#[specta::specta]
pub fn licenses_text() -> String {
    THIRD_PARTY.to_string()
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DiagnosticsSaved {
    pub path: String,
    pub bytes: u32,
}

/// Writes the diagnostics zip (logs, versions, OS and hardware, engine status, redacted settings) to `dest`.
#[tauri::command(async)]
#[specta::specta]
pub fn diagnostics_save(st: State<'_, Shared>, dest: String) -> Result<DiagnosticsSaved> {
    let mut path = crate::state::resolve_vault_path(&dest)?;
    if path.extension().is_none() {
        path.set_extension("zip");
    }
    let report = lowk_platform::hw::probe_report();
    let recommendation = lowk_platform::hw::recommend(&report);
    let settings = st.settings.get();
    let engines = st.engines.infos(&report, &settings.stt.cloud_vendor);
    let logs = lowk_platform::dirs::logs().unwrap_or_default();
    let read = |n: &str| std::fs::read_to_string(logs.join(n)).ok();
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let r = crate::diagnostics::Report {
        version: env!("CARGO_PKG_VERSION").into(),
        commit: env!("LOWK_GIT_COMMIT").into(),
        build_date: env!("LOWK_BUILD_DATE").into(),
        hw: HwProbe {
            report,
            recommendation,
        },
        engines: serde_json::to_value(&engines).unwrap_or_default(),
        settings: serde_json::to_value(&settings).unwrap_or_default(),
        log: read("lowk.log"),
        prev_log: read("lowk.prev.log"),
        home,
        secrets: lowk_platform::secrets::cached_values(),
        created,
    };
    let bytes = crate::diagnostics::write_zip(&path, &r)?;
    Ok(DiagnosticsSaved {
        path: path.display().to_string(),
        bytes: bytes.min(u64::from(u32::MAX)) as u32,
    })
}

/// Opens (or focuses) the main window; used by the second-instance handler,
/// the Dock reopen event and the overlay.
#[tauri::command]
#[specta::specta]
pub fn main_open(st: State<'_, Shared>) {
    crate::windows::open_main(&st.app);
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
    fn about_info_is_complete() {
        let a = about_info();
        assert_eq!(a.version, env!("CARGO_PKG_VERSION"));
        assert!(
            !a.commit.is_empty() && !a.build_date.is_empty(),
            "build.rs falls back to `unknown`, never empty"
        );
        assert!(a.build_date == "unknown" || (a.build_date.len() == 10 && a.build_date.starts_with("20")));
        assert!(a.license.starts_with("AGPL-3.0"));
        assert!(a.source_url.starts_with("https://"));
        assert!(licenses_text().contains("Third-party components"));
    }

    #[test]
    fn reveal_of_a_missing_path_is_an_error() {
        assert!(reveal(std::path::Path::new("/definitely/not/here/lowk")).is_err());
    }
}
