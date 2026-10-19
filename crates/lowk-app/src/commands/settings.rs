use lowk_types::{AppError, Settings, SettingsPatch, SettingsStore};
use std::sync::Arc;
use tauri::State;

pub type Store = Arc<dyn SettingsStore>;

#[tauri::command]
#[specta::specta]
pub fn settings_get(store: State<'_, Store>) -> Settings {
    store.get()
}

#[tauri::command]
#[specta::specta]
pub fn settings_patch(store: State<'_, Store>, patch: SettingsPatch) -> Result<Settings, AppError> {
    store.patch(patch)
}

/// Resolved UI locale (`ru` | `en`). M0 stub: real detection is `lowk-platform` (WP5).
#[tauri::command]
#[specta::specta]
pub fn locale_get() -> String {
    "en".to_string()
}
