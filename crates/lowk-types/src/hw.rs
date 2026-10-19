//! Hardware probe result (`lowk-platform::hw::probe`).
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GpuInfo {
    pub vendor: String,
    pub name: String,
    pub vram_mb: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct HwInfo {
    pub os: Os,
    pub os_version: String,
    pub arch: String,
    pub chip: String,
    pub ram_gb: f32,
    pub gpus: Vec<GpuInfo>,
    pub cpu_features: Vec<String>,
    pub free_disk_gb: f32,
    pub on_battery: bool,
}
