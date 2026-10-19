//! Stealth: WindowPolicy, StealthGuard, ModeMachine, magnet, panic hide, tone.
//! M0 stub (WP2 owns it). Only the public surface exists; the controller is a
//! no-op that reports `Unavailable`, so nothing can claim to be protected.
use lowk_types::fakes::NoopOverlayController;
use lowk_types::{MainThread, OverlayController, Result, Tone};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowRole {
    Overlay,
    Main,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowPolicy {
    pub role: WindowRole,
    pub capture_excluded: bool,
    pub hidden_from_switchers: bool,
    pub activates: bool,
}

impl WindowPolicy {
    pub const OVERLAY: WindowPolicy = WindowPolicy {
        role: WindowRole::Overlay,
        capture_excluded: true,
        hidden_from_switchers: true,
        activates: false,
    };
    pub fn main(hide_main: bool) -> WindowPolicy {
        WindowPolicy {
            role: WindowRole::Main,
            capture_excluded: hide_main,
            hidden_from_switchers: false,
            activates: true,
        }
    }
}

/// Native handles of the two labelled windows (placeholder; WP2 defines it).
#[derive(Default)]
pub struct RawWindows;

#[derive(Default, Clone, Copy)]
pub struct StealthConfig {
    pub hide_main: bool,
}

pub struct Stealth {
    pub controller: Arc<dyn OverlayController>,
}

pub fn create_stealth(
    _win: RawWindows,
    _main_thread: Arc<dyn MainThread>,
    _cfg: StealthConfig,
    _tone: Box<dyn Fn(Tone) + Send>,
) -> Result<Stealth> {
    Ok(Stealth {
        controller: Arc::new(NoopOverlayController::default()),
    })
}
