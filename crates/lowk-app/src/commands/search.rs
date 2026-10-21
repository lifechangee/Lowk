//! Search, index and embedder (M2): glue over `m2::Services` (`lowk-index`).
//! Events: `index://status` (`IndexStatus`), `engine://progress` with `engine == "embedder"`.
use crate::state::Shared;
use lowk_types::*;
use tauri::{Manager, State};

/// One search over calls, documents and screenshots.
#[tauri::command(async)]
#[specta::specta]
pub fn search_query(st: State<'_, Shared>, query: SearchQuery) -> Result<SearchResults> {
    st.svc()?.index.search(&query)
}

/// Counters and semantic state.
#[tauri::command(async)]
#[specta::specta]
pub fn index_status(st: State<'_, Shared>) -> Result<IndexStatus> {
    Ok(st.svc()?.index.status())
}

/// Queues a (re)index; the index is always rebuildable from the vault.
#[tauri::command(async)]
#[specta::specta]
pub fn index_rebuild(st: State<'_, Shared>, scope: ReindexScope) -> Result<()> {
    let svc = st.svc()?;
    svc.index.reindex(scope)?;
    svc.request_reconcile();
    Ok(())
}

/// Local embedder model state.
#[tauri::command(async)]
#[specta::specta]
pub fn embedder_info(st: State<'_, Shared>) -> Result<EmbedderInfo> {
    Ok(st.svc()?.embedder_info())
}

/// Downloads the embedder (progress via `engine://progress`).
#[tauri::command(async)]
#[specta::specta]
pub fn embedder_install(st: State<'_, Shared>) -> Result<()> {
    st.svc()?.embedder_install()
}

/// Cancels the download.
#[tauri::command(async)]
#[specta::specta]
pub fn embedder_install_cancel(st: State<'_, Shared>) -> Result<()> {
    st.svc()?.embedder_install_cancel();
    Ok(())
}

/// The overlay's source chip was clicked: show the source (call at `at_s`, document at `page`) in the
/// main window. The main window routes `nav://source` to the right page.
#[tauri::command(async)]
#[specta::specta]
pub fn source_open(st: State<'_, Shared>, source: SourceRef) -> Result<()> {
    let existed = st.app.get_webview_window("main").is_some();
    crate::windows::open_main(&st.app);
    let payload = serde_json::to_value(&source).unwrap_or_default();
    let st2: Shared = st.inner().clone();
    // a window that has to be created needs time to load its listeners
    let delay = if existed { 120 } else { 1500 };
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay));
        st2.emitter.to_main("nav://source", payload);
    });
    Ok(())
}

/// Opens the main window on a Settings page (the overlay's «no API key» message links here).
#[tauri::command(async)]
#[specta::specta]
pub fn settings_open(st: State<'_, Shared>, page: String) -> Result<()> {
    let existed = st.app.get_webview_window("main").is_some();
    crate::windows::open_main(&st.app);
    let st2: Shared = st.inner().clone();
    let delay = if existed { 120 } else { 1500 };
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay));
        st2.emitter
            .to_main("nav://settings", serde_json::json!({ "page": page }));
    });
    Ok(())
}
