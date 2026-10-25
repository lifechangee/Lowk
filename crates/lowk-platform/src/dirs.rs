//! Application directories and the data-root policy.
//!
//! Two roots:
//! - `app_data` (`%APPDATA%\Lowk`, `~/Library/Application Support/Lowk`): small per-user state, `settings.toml`
//!   and the pointer to the data root. Secrets live in the OS keychain.
//! - `data_root`: large data (STT/embedder models, engines, logs). Windows default: `<install dir>\data` when
//!   that is writable per-user, else `%LOCALAPPDATA%\Lowk`; macOS keeps `app_data` (bundles stay sealed). The user
//!   can move it (Settings -> Storage); the choice is persisted in `<app_data>/data-root.txt`.
//!
//! Every path the app writes to is a method of [`Layout`]; the uninstall purge (`purge.rs`) is generated from the
//! same list, so the two cannot drift.
use directories::BaseDirs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Tauri bundle identifier (`tauri.conf.json`); also the keychain service and the WebView2 profile folder name.
pub const BUNDLE_ID: &str = "dev.lowk.app";
const APP_DIR: &str = "Lowk";
const POINTER_FILE: &str = "data-root.txt";
/// Sub-folders of the data root.
pub const ROOT_SUBDIRS: [&str; 3] = ["models", "engines", "logs"];

/// All locations for one (app data, data root) pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub app_data: PathBuf,
    pub data_root: PathBuf,
}

impl Layout {
    pub fn new(app_data: PathBuf, data_root: PathBuf) -> Self {
        Self { app_data, data_root }
    }
    pub fn models(&self) -> PathBuf {
        self.data_root.join("models")
    }
    pub fn engines(&self) -> PathBuf {
        self.data_root.join("engines")
    }
    pub fn logs(&self) -> PathBuf {
        self.data_root.join("logs")
    }
    pub fn settings_file(&self) -> PathBuf {
        self.app_data.join("settings.toml")
    }
    pub fn pointer_file(&self) -> PathBuf {
        self.app_data.join(POINTER_FILE)
    }
    pub fn log_file(&self) -> PathBuf {
        self.logs().join("lowk.log")
    }
    pub fn prev_log_file(&self) -> PathBuf {
        self.logs().join("lowk.prev.log")
    }
    /// Every file or folder the app creates, for tests and the purge coverage check.
    pub fn written_paths(&self) -> Vec<PathBuf> {
        vec![
            self.app_data.clone(),
            self.settings_file(),
            self.app_data.join("settings.toml.bad"),
            self.pointer_file(),
            self.app_data.join("qa.json"),
            self.models(),
            self.engines(),
            self.logs(),
            self.log_file(),
            self.prev_log_file(),
        ]
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Settings / pointer directory. `LOWK_DATA_DIR` isolates a whole profile (QA, tests).
pub fn app_data() -> Option<PathBuf> {
    env_path("LOWK_DATA_DIR").or_else(|| BaseDirs::new().map(|d| d.data_dir().join(APP_DIR)))
}

/// `%LOCALAPPDATA%\Lowk` (Windows fallback data root; other platforms: `app_data`).
pub fn local_fallback() -> Option<PathBuf> {
    if cfg!(windows) {
        BaseDirs::new().map(|d| d.data_local_dir().join(APP_DIR))
    } else {
        app_data()
    }
}

/// Can we create files in `dir` (creating it if needed)? Leaves nothing behind.
pub fn dir_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".lowk-write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// The default data root. Pure apart from the writability probe.
pub fn default_root(app_data: &Path, exe_dir: Option<&Path>, local: Option<&Path>, windows: bool) -> PathBuf {
    if windows {
        if let Some(d) = exe_dir {
            let c = d.join("data");
            if dir_writable(&c) {
                return c;
            }
        }
        if let Some(l) = local {
            return l.to_path_buf();
        }
    }
    app_data.to_path_buf()
}

/// What `data_root` would be with nothing configured.
pub fn default_data_root() -> Option<PathBuf> {
    let app = app_data()?;
    // An isolated profile (LOWK_DATA_DIR) keeps everything inside it.
    if env_path("LOWK_DATA_DIR").is_some() {
        return Some(app);
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    Some(default_root(
        &app,
        exe_dir.as_deref(),
        local_fallback().as_deref(),
        cfg!(windows),
    ))
}

pub fn read_pointer(app_data: &Path) -> Option<PathBuf> {
    let s = std::fs::read_to_string(app_data.join(POINTER_FILE)).ok()?;
    let p = PathBuf::from(s.trim());
    (p.is_absolute()).then_some(p)
}

pub fn write_pointer(app_data: &Path, root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(app_data)?;
    std::fs::write(app_data.join(POINTER_FILE), root.to_string_lossy().as_bytes())
}

static ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// The active data root (explicit `LOWK_DATA_ROOT`, else the persisted pointer, else the default).
pub fn data_root() -> Option<PathBuf> {
    if let Some(p) = env_path("LOWK_DATA_ROOT") {
        return Some(p);
    }
    if let Some(p) = ROOT.read().ok().and_then(|g| g.clone()) {
        return Some(p);
    }
    let app = app_data()?;
    let r = if env_path("LOWK_DATA_DIR").is_some() {
        None
    } else {
        read_pointer(&app)
    }
    .or_else(default_data_root)?;
    if let Ok(mut g) = ROOT.write() {
        *g = Some(r.clone());
    }
    Some(r)
}

/// Makes `root` the data root for this and future runs.
pub fn set_data_root(root: &Path) -> std::io::Result<()> {
    if let Some(app) = app_data() {
        write_pointer(&app, root)?;
    }
    if let Ok(mut g) = ROOT.write() {
        *g = Some(root.to_path_buf());
    }
    Ok(())
}

/// Switches the root for this process only (a background move is still running; nothing is persisted).
pub fn use_root_for_now(root: &Path) {
    if let Ok(mut g) = ROOT.write() {
        *g = Some(root.to_path_buf());
    }
}

/// The current layout.
pub fn layout() -> Option<Layout> {
    Some(Layout::new(app_data()?, data_root()?))
}

pub fn models() -> Option<PathBuf> {
    layout().map(|l| l.models())
}

pub fn engines() -> Option<PathBuf> {
    layout().map(|l| l.engines())
}

pub fn logs() -> Option<PathBuf> {
    layout().map(|l| l.logs())
}

/// Default vault location `~/Documents/Lowk` (`None` on exotic setups).
pub fn default_vault() -> Option<PathBuf> {
    directories::UserDirs::new().and_then(|d| d.document_dir().map(|p| p.join(APP_DIR)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_paths() {
        let l = Layout::new(PathBuf::from("/a"), PathBuf::from("/r"));
        assert_eq!(l.models(), PathBuf::from("/r/models"));
        assert_eq!(l.log_file(), PathBuf::from("/r/logs/lowk.log"));
        assert_eq!(l.settings_file(), PathBuf::from("/a/settings.toml"));
        for s in ROOT_SUBDIRS {
            assert!(l.written_paths().contains(&l.data_root.join(s)));
        }
    }

    #[test]
    fn default_root_windows_prefers_writable_install_dir() {
        let t = tempfile::tempdir().unwrap();
        let app = t.path().join("roaming/Lowk");
        let exe = t.path().join("Programs/Lowk");
        let local = t.path().join("local/Lowk");
        std::fs::create_dir_all(&exe).unwrap();
        assert_eq!(
            default_root(&app, Some(&exe), Some(&local), true),
            exe.join("data")
        );
        assert!(!exe.join(".lowk-write-test-0").exists());
    }

    #[test]
    fn default_root_falls_back_to_local_when_install_dir_unwritable() {
        let t = tempfile::tempdir().unwrap();
        let app = t.path().join("roaming/Lowk");
        let local = t.path().join("local/Lowk");
        // a *file* where the install dir should be: `data` can never be created below it
        let exe = t.path().join("blocked");
        std::fs::write(&exe, b"x").unwrap();
        assert_eq!(default_root(&app, Some(&exe), Some(&local), true), local);
        assert_eq!(default_root(&app, None, Some(&local), true), local);
    }

    #[test]
    fn default_root_non_windows_keeps_app_data() {
        let t = tempfile::tempdir().unwrap();
        let app = t.path().join("Application Support/Lowk");
        assert_eq!(default_root(&app, Some(t.path()), Some(t.path()), false), app);
    }

    #[test]
    fn pointer_roundtrip_and_validation() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(read_pointer(t.path()), None);
        let root = t.path().join("big disk");
        write_pointer(t.path(), &root).unwrap();
        assert_eq!(read_pointer(t.path()), Some(root));
        std::fs::write(t.path().join(POINTER_FILE), "relative/path").unwrap();
        assert_eq!(read_pointer(t.path()), None, "relative pointers are ignored");
    }
}
