//! Search, index and embedder (M2, stubs until WP-E wires `lowk-index`).
//! Events: `index://status` (`IndexStatus`), `engine://progress` with `engine == "embedder"`.
use lowk_types::*;

/// One search over calls, documents and screenshots.
#[tauri::command(async)]
#[specta::specta]
pub fn search_query(_query: SearchQuery) -> Result<SearchResults> {
    Err(AppError::not_implemented())
}

/// Counters and semantic state.
#[tauri::command(async)]
#[specta::specta]
pub fn index_status() -> Result<IndexStatus> {
    Err(AppError::not_implemented())
}

/// Queues a (re)index; the index is always rebuildable from the vault.
#[tauri::command(async)]
#[specta::specta]
pub fn index_rebuild(_scope: ReindexScope) -> Result<()> {
    Err(AppError::not_implemented())
}

/// Local embedder model state.
#[tauri::command(async)]
#[specta::specta]
pub fn embedder_info() -> Result<EmbedderInfo> {
    Err(AppError::not_implemented())
}

/// Downloads the embedder (progress via `engine://progress`).
#[tauri::command(async)]
#[specta::specta]
pub fn embedder_install() -> Result<()> {
    Err(AppError::not_implemented())
}

/// Cancels the download.
#[tauri::command(async)]
#[specta::specta]
pub fn embedder_install_cancel() -> Result<()> {
    Err(AppError::not_implemented())
}
