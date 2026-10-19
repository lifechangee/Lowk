use tauri::WebviewWindow;
pub fn setup_overlay(_w: &WebviewWindow) {}
pub fn after_show(_w: &WebviewWindow) {}
pub fn apply_exclusion(_w: &WebviewWindow) -> String { "unsupported platform".into() }
pub fn set_excluded_default(_on: bool) {}
pub fn observe_space_changes(_h: tauri::AppHandle, _f: fn(&tauri::AppHandle)) {}
