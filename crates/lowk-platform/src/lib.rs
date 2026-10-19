//! OS probing: permissions, keychain secrets, hardware probe, locale, app dirs.
//! M0 stub (WP5 owns the real implementation). Signatures follow section 10.
use lowk_types::{AppError, HwInfo, PermKind, PermState, Result};

pub mod perm {
    use super::*;
    pub fn status(_k: PermKind) -> PermState {
        PermState::Unknown
    }
    pub fn request(_k: PermKind) -> Result<PermState> {
        Err(AppError::not_implemented())
    }
    pub fn open_settings(_k: PermKind) {}
}

pub mod secrets {
    use super::*;
    pub fn set(_provider: &str, _value: &str) -> Result<()> {
        Err(AppError::not_implemented())
    }
    pub fn present(_provider: &str) -> bool {
        false
    }
    /// Rust-only: the value never reaches the WebView.
    pub fn get(_provider: &str) -> Result<String> {
        Err(AppError::not_implemented())
    }
    pub fn delete(_provider: &str) -> Result<()> {
        Err(AppError::not_implemented())
    }
}

pub mod hw {
    use super::*;
    pub fn probe() -> HwInfo {
        HwInfo {
            os: lowk_types::Os::current(),
            os_version: String::new(),
            arch: std::env::consts::ARCH.to_string(),
            chip: String::new(),
            ram_gb: 0.0,
            gpus: vec![],
            cpu_features: vec![],
            free_disk_gb: 0.0,
            on_battery: false,
        }
    }
}

pub mod locale {
    /// System locale string (e.g. `ru-RU`); the stub reports English.
    pub fn system() -> String {
        "en-US".to_string()
    }
}
