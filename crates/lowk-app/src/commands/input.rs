//! binds: read, write, record.
use crate::state::{AppState, Shared};
use lowk_types::*;
use std::time::Duration;
use tauri::State;

fn state(st: &AppState) -> BindsState {
    BindsState {
        binds: st.settings.get().binds,
        report: st.binds_report.lock().clone(),
    }
}

#[tauri::command(async)]
#[specta::specta]
pub fn binds_get(st: State<'_, Shared>) -> BindsState {
    state(&st)
}

#[tauri::command(async)]
#[specta::specta]
pub fn binds_set(st: State<'_, Shared>, binds: Vec<BindSpec>) -> Result<BindsState> {
    for b in &binds {
        // A template bind must name its template (any id: built-in or user-created).
        if b.action == ActionId::AskTemplate && b.template.as_deref().is_none_or(str::is_empty) {
            return Err(
                AppError::new(ErrorCode::InvalidArgument).with_detail("ask_template needs a template")
            );
        }
        if let Trigger::Combo { key, .. } = &b.trigger {
            if !lowk_input::keys::is_known_code(&key.0) {
                return Err(
                    AppError::new(ErrorCode::InvalidArgument).with_detail(format!("unknown key {}", key.0))
                );
            }
        }
    }
    let v = serde_json::to_value(&binds)
        .map_err(|e| AppError::new(ErrorCode::SettingsInvalid).with_detail(e.to_string()))?;
    st.settings
        .patch(SettingsPatch(serde_json::json!({ "binds": v })))?;
    crate::boot::apply_binds(&st);
    Ok(state(&st))
}

/// Captures the next combo / lone modifier / mouse button through the
/// low-level hook (Input Monitoring on macOS). `None` = timed out or stopped.
#[tauri::command(async)]
#[specta::specta]
pub async fn binds_record_start(st: State<'_, Shared>) -> Result<Option<Trigger>> {
    let rx = st.input.lock().record_start()?;
    let r = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(20)).ok())
        .await
        .map_err(|e| AppError::new(ErrorCode::Unknown).with_detail(e.to_string()))?;
    st.input.lock().record_stop();
    Ok(r)
}

#[tauri::command(async)]
#[specta::specta]
pub fn binds_record_stop(st: State<'_, Shared>) {
    st.input.lock().record_stop();
}
