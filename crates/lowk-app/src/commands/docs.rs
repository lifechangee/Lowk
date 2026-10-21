//! Documents (M2): thin glue over `m2::Services` (ingest queue, vault documents, index feed).
//! Events: `doc://progress` (`IngestProgress`).
use crate::state::Shared;
use lowk_types::*;
use tauri::State;

fn changed(st: &Shared, kind: StoreChangeKind, id: Ulid) {
    st.emit(AppEvent::Store(StoreChange {
        kind,
        id,
        is_folder: false,
    }));
}

/// Documents of a folder (not trashed).
#[tauri::command(async)]
#[specta::specta]
pub fn doc_list(st: State<'_, Shared>, folder: FolderRef) -> Result<Vec<Document>> {
    st.svc()?.vault.list_docs(&folder)
}

/// One document.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_get(st: State<'_, Shared>, id: DocId) -> Result<Document> {
    st.svc()?.vault.get_doc(&id)
}

/// Imports files (drop or dialog). Returns the queued documents at once; conversion runs in the background and reports `doc://progress`.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_import(st: State<'_, Shared>, files: Vec<NewDocument>) -> Result<Vec<Document>> {
    st.svc()?.import_docs(&files)
}

/// Converted Markdown (editor, viewer).
#[tauri::command(async)]
#[specta::specta]
pub fn doc_get_md(st: State<'_, Shared>, id: DocId) -> Result<String> {
    st.svc()?.vault.read_doc_md(&id)
}

/// Saves the editor text; re-index follows.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_save_md(st: State<'_, Shared>, id: DocId, md: String) -> Result<Document> {
    let d = st.svc()?.vault.save_doc_md(&id, &md)?;
    changed(&st, StoreChangeKind::Updated, id);
    Ok(d)
}

/// Title, tags, pinned, move to another folder.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_patch(st: State<'_, Shared>, id: DocId, patch: DocPatch) -> Result<Document> {
    let d = st.svc()?.vault.patch_doc(&id, &patch)?;
    changed(&st, if patch.folder.is_some() { StoreChangeKind::Moved } else { StoreChangeKind::Updated }, id);
    Ok(d)
}

/// Moves to the Trash (restorable).
#[tauri::command(async)]
#[specta::specta]
pub fn doc_delete(st: State<'_, Shared>, id: DocId) -> Result<()> {
    let svc = st.svc()?;
    // a running conversion is stopped first, so it cannot write into a trashed document
    let _ = svc.cancel_doc(&id);
    svc.vault.delete_doc(&id)?;
    changed(&st, StoreChangeKind::Trashed, id);
    Ok(())
}

/// Restores from the Trash.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_restore(st: State<'_, Shared>, id: DocId) -> Result<Document> {
    let d = st.svc()?.vault.restore_doc(&id)?;
    changed(&st, StoreChangeKind::Restored, id);
    Ok(d)
}

/// Re-runs a failed or cancelled conversion.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_retry(st: State<'_, Shared>, id: DocId) -> Result<Document> {
    st.svc()?.retry_doc(&id)
}

/// Cancels a queued or running conversion.
#[tauri::command(async)]
#[specta::specta]
pub fn doc_cancel(st: State<'_, Shared>, id: DocId) -> Result<()> {
    st.svc()?.cancel_doc(&id)
}

/// Pinned-documents budget meter of a folder.
#[tauri::command(async)]
#[specta::specta]
pub fn docs_pinned_usage(st: State<'_, Shared>, folder: FolderRef) -> Result<PinnedUsage> {
    let budget = st.settings.get().docs.pinned_budget_tokens;
    st.svc()?.vault.pinned_usage(&folder, budget)
}
