//! OS permissions: `status` / `request` / `open_settings` for every `PermKind`.
//!
//! * `status` never prompts and never changes anything.
//! * `request` may show the OS prompt (Screen Recording, Microphone, Input
//!   Monitoring). Microphone blocks until the user answers, so call it from
//!   `spawn_blocking`. It always returns the state observed afterwards; the OS
//!   often applies a grant only after an app restart (Screen Recording, Input
//!   Monitoring), so callers should re-poll `status` on window focus.
//! * `open_settings` opens the matching System Settings pane.
//!
//! macOS System Audio Recording has no public status or request API (TCC service
//! `kTCCServiceAudioCapture`; it is only evaluated when a process tap is
//! created). Heuristic: `status(SystemAudio)` reports what the last probe tap
//! observed, fed in by `lowk-audio` through [`report_system_audio_probe`]:
//! tap creation failed -> `Denied`; tap delivered non-zero samples ->
//! `Granted`; only digital silence (or no probe yet) -> `Unknown`, because
//! silence is also what a denied tap or a quiet system produces.
//!
//! `request(SystemAudio)` cannot prompt from here (this crate does not know about
//! audio): the real flow lives in `lowk-app::perm_request`, which runs
//! `lowk_audio::probe_system_audio` (a short tap on a quiet tone we play ourselves; creating
//! the tap raises the genuine macOS prompt), and, when the verdict is `Denied`, opens
//! [`settings_url`]. Verdicts reach [`set_change_hook`] subscribers, which emit `perm://changed`;
//! live capture also reports `Working` as soon as real audio arrives.
use lowk_types::{PermKind, PermState, Result};
use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as os;

#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as os;

#[cfg(not(any(target_os = "macos", windows)))]
mod other;
#[cfg(not(any(target_os = "macos", windows)))]
use other as os;

/// Outcome of a system-audio probe tap, reported by `lowk-audio`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemAudioProbe {
    /// Tap created and delivered non-zero samples.
    Working,
    /// Tap creation / start failed with a permission-type error.
    Failed,
    /// Tap works but only silence arrived (inconclusive).
    Silent,
}

static SYSTEM_AUDIO: AtomicU8 = AtomicU8::new(0);

type ChangeHook = Box<dyn Fn(PermKind, PermState) + Send + Sync>;
static HOOK: std::sync::OnceLock<ChangeHook> = std::sync::OnceLock::new();

/// Registers the (single) subscriber told when a permission state changes by itself
/// (today: `SystemAudio`, from probes and live capture). First registration wins.
pub fn set_change_hook(f: ChangeHook) {
    let _ = HOOK.set(f);
}

pub fn report_system_audio_probe(p: SystemAudioProbe) {
    let new = match p {
        SystemAudioProbe::Silent => 0,
        SystemAudioProbe::Working => 1,
        SystemAudioProbe::Failed => 2,
    };
    let old = SYSTEM_AUDIO.swap(new, Ordering::Relaxed);
    if old != new {
        if let Some(h) = HOOK.get() {
            h(PermKind::SystemAudio, system_audio_heuristic());
        }
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn system_audio_heuristic() -> PermState {
    match SYSTEM_AUDIO.load(Ordering::Relaxed) {
        1 => PermState::Granted,
        2 => PermState::Denied,
        _ => PermState::Unknown,
    }
}

pub fn status(k: PermKind) -> PermState {
    os::status(k)
}

pub fn request(k: PermKind) -> Result<PermState> {
    os::request(k)
}

pub fn open_settings(k: PermKind) {
    if let Some(url) = settings_url(k) {
        os::open_url(url);
    }
}

/// Deep link for the settings pane of `k` on this OS (`None` = no pane exists).
pub fn settings_url(k: PermKind) -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        Some(match k {
            PermKind::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            PermKind::SystemAudio => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture"
            }
            PermKind::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            PermKind::InputMonitoring => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
            }
            PermKind::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        match k {
            PermKind::Microphone if cfg!(windows) => Some("ms-settings:privacy-microphone"),
            _ => None,
        }
    }
}

/// All kinds, in onboarding order.
pub const ALL: [PermKind; 5] = [
    PermKind::Microphone,
    PermKind::SystemAudio,
    PermKind::ScreenRecording,
    PermKind::InputMonitoring,
    PermKind::Accessibility,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic() {
        report_system_audio_probe(SystemAudioProbe::Working);
        assert_eq!(system_audio_heuristic(), PermState::Granted);
        report_system_audio_probe(SystemAudioProbe::Failed);
        assert_eq!(system_audio_heuristic(), PermState::Denied);
        report_system_audio_probe(SystemAudioProbe::Silent);
        assert_eq!(system_audio_heuristic(), PermState::Unknown);
    }

    #[test]
    fn links() {
        for k in ALL {
            let u = settings_url(k);
            if cfg!(target_os = "macos") {
                assert!(u.unwrap().starts_with("x-apple.systempreferences:"));
            }
        }
    }
}
