//! Update flow (see `crate::updater` for the policy). Every command is a no-op without an update key.
use crate::state::Shared;
use crate::updater::{self, UpdatePhase, UpdateStatus, Updates};
use lowk_types::*;
use std::sync::Arc;
use tauri::State;

fn status(st: &Shared, ups: &Updates) -> UpdateStatus {
    ups.status(st.settings.get().updates.last_checked)
}

/// Current update state; cheap, polled by the main window.
#[tauri::command]
#[specta::specta]
pub fn update_status(st: State<'_, Shared>, ups: State<'_, Arc<Updates>>) -> UpdateStatus {
    status(&st, &ups)
}

/// «Проверить сейчас» (`manual`: ignores the toggle and the daily throttle, not the recording rule).
#[tauri::command]
#[specta::specta]
pub async fn update_check(
    st: State<'_, Shared>,
    ups: State<'_, Arc<Updates>>,
    manual: bool,
) -> Result<UpdateStatus> {
    let (st, ups) = (st.inner().clone(), ups.inner().clone());
    updater::run_check(&st, &ups, manual).await;
    Ok(status(&st, &ups))
}

/// «Обновить»: downloads and verifies in the background (poll `update_status`). Nothing is installed
/// yet; the user then picks «Перезапустить» (or «Перезапустить после записи»).
#[tauri::command]
#[specta::specta]
pub fn update_install(st: State<'_, Shared>, ups: State<'_, Arc<Updates>>) -> UpdateStatus {
    let (st, ups) = (st.inner().clone(), ups.inner().clone());
    if ups.key_ok() && ups.phase() == UpdatePhase::Available {
        let u2 = ups.clone();
        tauri::async_runtime::spawn(async move { updater::download(&u2).await });
    }
    status(&st, &ups)
}

/// «Перезапустить» for a downloaded update: now, or right after the recording if one is running.
#[tauri::command]
#[specta::specta]
pub fn update_restart(st: State<'_, Shared>, ups: State<'_, Arc<Updates>>) -> UpdateStatus {
    let (st, ups) = (st.inner().clone(), ups.inner().clone());
    ups.request_restart();
    updater::apply(&st, &ups);
    status(&st, &ups)
}

/// «Пропустить версию».
#[tauri::command]
#[specta::specta]
pub fn update_skip(
    st: State<'_, Shared>,
    ups: State<'_, Arc<Updates>>,
    version: String,
) -> Result<UpdateStatus> {
    st.settings.patch(SettingsPatch(
        serde_json::json!({ "updates": { "skipped_version": version } }),
    ))?;
    if ups.offered_version().as_deref() == Some(version.as_str())
        && matches!(ups.phase(), UpdatePhase::Available | UpdatePhase::Error)
    {
        ups.clear();
    }
    Ok(status(&st, &ups))
}
