//! Export / import of one folder as a zip (M2). Whole-vault export/import already exist in `vault.rs`.
use lowk_types::*;

/// Writes a zip of the folder (calls, documents, instructions; no `.lowk/`). Returns files written.
#[tauri::command(async)]
#[specta::specta]
pub fn folder_export(_id: FolderId, _dest: String) -> Result<u32> {
    Err(AppError::not_implemented())
}

/// Imports a folder/vault zip (ids reassigned on collision, never overwrites).
#[tauri::command(async)]
#[specta::specta]
pub fn folder_import(_src: String) -> Result<crate::commands::vault::ImportSummary> {
    Err(AppError::not_implemented())
}
