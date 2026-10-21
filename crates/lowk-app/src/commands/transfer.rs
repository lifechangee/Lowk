//! Export / import of one folder as a zip (M2). Whole-vault export/import (directory or zip) is in `vault.rs`.
use crate::commands::vault::ImportSummary;
use crate::state::Shared;
use lowk_types::*;
use std::path::PathBuf;
use tauri::State;

/// Writes a zip of the folder (calls, documents, instructions; no `.lowk/`). Returns files written.
#[tauri::command(async)]
#[specta::specta]
pub fn folder_export(st: State<'_, Shared>, id: FolderId, dest: String) -> Result<u32> {
    Ok(st.vault()?.export_folder_zip(&id, &PathBuf::from(dest))? as u32)
}

/// Imports a folder/vault zip (ids reassigned on collision, never overwrites).
#[tauri::command(async)]
#[specta::specta]
pub fn folder_import(st: State<'_, Shared>, src: String) -> Result<ImportSummary> {
    crate::commands::vault::import_any(&st, &PathBuf::from(src))
}
