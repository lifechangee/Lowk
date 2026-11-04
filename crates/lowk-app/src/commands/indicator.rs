use crate::indicator;
use lowk_types::Indicator;

#[tauri::command]
#[specta::specta]
pub fn indicator_get() -> Indicator {
    indicator::current()
}
