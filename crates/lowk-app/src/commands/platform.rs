//! permissions, hardware.
use lowk_types::*;
use std::collections::HashMap;

#[tauri::command(async)]
#[specta::specta]
pub fn perm_status() -> HashMap<PermKind, PermState> {
    use lowk_platform::perm;
    perm::ALL.iter().map(|k| (*k, perm::status(*k))).collect()
}

#[tauri::command(async)]
#[specta::specta]
pub async fn perm_request(kind: PermKind) -> Result<PermState> {
    // Microphone blocks until the user answers (up to minutes): keep it off the runtime threads.
    tauri::async_runtime::spawn_blocking(move || lowk_platform::perm::request(kind))
        .await
        .map_err(|e| AppError::new(ErrorCode::Unknown).with_detail(e.to_string()))?
}

#[tauri::command(async)]
#[specta::specta]
pub fn perm_open_settings(kind: PermKind) {
    lowk_platform::perm::open_settings(kind);
}

#[tauri::command(async)]
#[specta::specta]
pub async fn hw_probe() -> Result<HwProbe> {
    tauri::async_runtime::spawn_blocking(|| {
        let report = lowk_platform::hw::probe_report();
        let recommendation = lowk_platform::hw::recommend(&report);
        HwProbe {
            report,
            recommendation,
        }
    })
    .await
    .map_err(|e| AppError::new(ErrorCode::Unknown).with_detail(e.to_string()))
}

/// 5-10 s engine benchmark: arrives with the first real engines (M1).
#[tauri::command(async)]
#[specta::specta]
pub fn hw_bench() -> Result<()> {
    Err(AppError::not_implemented())
}
