//! Application directories. Layout follows docs/architecture.md:
//! macOS `~/Library/Application Support/Lowk`, Windows `%APPDATA%\Lowk`.
use directories::BaseDirs;
use std::path::PathBuf;

const APP_DIR: &str = "Lowk";

/// `<app data>`: settings.toml, state.json, models, engines, logs.
pub fn app_data() -> Option<PathBuf> {
    BaseDirs::new().map(|d| d.data_dir().join(APP_DIR))
}

pub fn models() -> Option<PathBuf> {
    app_data().map(|d| d.join("models"))
}

pub fn engines() -> Option<PathBuf> {
    app_data().map(|d| d.join("engines"))
}

pub fn logs() -> Option<PathBuf> {
    app_data().map(|d| d.join("logs"))
}

/// Default vault location `~/Documents/Lowk` (`None` on exotic setups).
pub fn default_vault() -> Option<PathBuf> {
    directories::UserDirs::new().and_then(|d| d.document_dir().map(|p| p.join(APP_DIR)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let d = app_data().expect("home dir");
        assert!(d.ends_with("Lowk"));
        assert!(models().unwrap().ends_with("models"));
        #[cfg(target_os = "macos")]
        assert!(d.to_string_lossy().contains("Library/Application Support"));
    }
}
