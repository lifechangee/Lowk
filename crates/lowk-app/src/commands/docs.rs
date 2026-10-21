//! Documents (M2, stubs until WP-E wires `lowk-store` + `lowk-ingest` + `lowk-index`).
//! Events: `doc://progress` (`IngestProgress`).
use lowk_types::*;

/// Documents of a folder (not trashed).
#[tauri::command(async)]
#[specta::specta]
pub fn doc_list(_folder: FolderRef) -> Result<Vec<Document>> {
    Err(AppError::not_implemented())
}

/// One document.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_get(_id: DocId) -> Result<Document> {
    Err(AppError::not_implemented())
}

/// Imports files (drop or dialog). Returns the queued documents at once; conversion runs in the background and reports `doc://progress`.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_import(_files: Vec<NewDocument>) -> Result<Vec<Document>> {
    Err(AppError::not_implemented())
}

/// Converted Markdown (editor, viewer).
#[tauri::command(async)]
#[specta::specta]
pub fn doc_get_md(_id: DocId) -> Result<String> {
    Err(AppError::not_implemented())
}

/// Saves the editor text; re-index follows.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_save_md(_id: DocId, _md: String) -> Result<Document> {
    Err(AppError::not_implemented())
}

/// Title, tags, pinned, move to another folder.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_patch(_id: DocId, _patch: DocPatch) -> Result<Document> {
    Err(AppError::not_implemented())
}

/// Moves to the Trash (restorable).
#[tauri::command(async)]
#[specta::specta]
pub fn doc_delete(_id: DocId) -> Result<()> {
    Err(AppError::not_implemented())
}

/// Restores from the Trash.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_restore(_id: DocId) -> Result<Document> {
    Err(AppError::not_implemented())
}

/// Re-runs a failed or cancelled conversion.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_retry(_id: DocId) -> Result<Document> {
    Err(AppError::not_implemented())
}

/// Cancels a queued or running conversion.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_cancel(_id: DocId) -> Result<()> {
    Err(AppError::not_implemented())
}

/// Pinned-documents budget meter of a folder.
#[tauri::command(async)]
#[specta::specta]
pub fn docs_pinned_usage(_folder: FolderRef) -> Result<PinnedUsage> {
    Err(AppError::not_implemented())
}
