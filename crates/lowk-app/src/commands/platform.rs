//! permissions, hardware.
use crate::state::Shared;
use lowk_types::*;
use std::collections::HashMap;
use tauri::State;

/// Forwards self-detected permission changes (system audio, from probes and live capture)
/// to the UI as `perm://changed`. Registered once, on first use of a perm command.
fn ensure_perm_hook(st: &Shared) {
    use lowk_types::EventSink;
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let em = st.emitter.clone();
        lowk_platform::perm::set_change_hook(Box::new(move |kind, state| {
            em.emit(AppEvent::Perm { kind, state })
        }));
    });
}

#[tauri::command(async)]
#[specta::specta]
pub fn perm_status(st: State<'_, Shared>) -> HashMap<PermKind, PermState> {
    use lowk_platform::perm;
    ensure_perm_hook(&st);
    perm::ALL.iter().map(|k| (*k, perm::status(*k))).collect()
}

/// macOS "System Audio Recording" has no request API. Run a short probe tap on a quiet tone
/// we play ourselves: creating the tap raises the real TCC prompt, and the buffers tell us
/// the answer (audio arrived = granted, silence while it plays = denied). Denied opens the
/// matching Settings pane. Inconclusive (nothing could be played) stays `Unknown`.
fn request_system_audio() -> Result<PermState> {
    use lowk_audio::SystemAudioProbe;
    use lowk_platform::perm;
    if !cfg!(target_os = "macos") {
        return Ok(perm::status(PermKind::SystemAudio));
    }
    match lowk_audio::probe_system_audio(std::time::Duration::from_secs(150)) {
        SystemAudioProbe::Working => Ok(PermState::Granted),
        SystemAudioProbe::Failed => {
            perm::open_settings(PermKind::SystemAudio);
            Ok(PermState::Denied)
        }
        SystemAudioProbe::Silent => Ok(PermState::Unknown),
        SystemAudioProbe::TapError => Err(AppError::new(ErrorCode::AudioDevice).with_detail(format!(
            "System audio capture could not start (not a permission problem): {}",
            perm::system_audio_reason()
        ))),
    }
}

#[tauri::command(async)]
#[specta::specta]
pub async fn perm_request(st: State<'_, Shared>, kind: PermKind) -> Result<PermState> {
    ensure_perm_hook(&st);
    let em = st.emitter.clone();
    // Microphone and system audio block until the user answers the prompt (up to minutes):
    // keep them off the runtime threads.
    tauri::async_runtime::spawn_blocking(move || {
        use lowk_types::EventSink;
        let before = lowk_platform::perm::status(kind);
        let after = if kind == PermKind::SystemAudio {
            request_system_audio()?
        } else {
            lowk_platform::perm::request(kind)?
        };
        if after != before {
            em.emit(AppEvent::Perm { kind, state: after });
        }
        Ok(after)
    })
    .await
    .map_err(|e| AppError::new(ErrorCode::Unknown).with_detail(e.to_string()))?
}

#[tauri::command(async)]
#[specta::specta]
pub fn perm_open_settings(kind: PermKind) {
    lowk_platform::perm::open_settings(kind);
}

#[tauri::command(async)]
#[specta::specta]
pub async fn hw_probe() -> Result<HwProbe> {
    tauri::async_runtime::spawn_blocking(|| {
        let report = lowk_platform::hw::probe_report();
        let recommendation = lowk_platform::hw::recommend(&report);
        HwProbe {
            report,
            recommendation,
        }
    })
    .await
    .map_err(|e| AppError::new(ErrorCode::Unknown).with_detail(e.to_string()))
}

/// 5-10 s engine benchmark: arrives with the first real engines (M1).
#[tauri::command(async)]
#[specta::specta]
pub fn hw_bench() -> Result<()> {
    Err(AppError::not_implemented())
}
