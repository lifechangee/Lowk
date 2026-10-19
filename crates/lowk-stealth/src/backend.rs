//! Native window operations behind a trait so the guard / controller logic is
//! testable without a window server. Every method must be called on the main
//! thread (the guard guarantees it through the injected `MainThread`).

use crate::mode::Interaction;
use crate::policy::WindowPolicy;
use lowk_types::{AppError, ErrorCode, Rect, Result};

/// What the OS reports back after a set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Readback {
    /// Capture exclusion flag as read from the OS (not the value we set).
    pub capture_excluded: bool,
    /// Cheap second opinion (macOS `kCGWindowSharingState == 0`), if available.
    pub window_server_excluded: Option<bool>,
    pub detail: String,
}

impl Readback {
    pub fn matches(&self, p: &WindowPolicy) -> bool {
        self.capture_excluded == p.capture_excluded
            && self
                .window_server_excluded
                .is_none_or(|w| w == p.capture_excluded)
    }
}

pub trait WindowBackend: Send + Sync {
    /// One-time setup before the first show (panel conversion, level,
    /// collection behaviour, tool-window styles).
    fn prepare(&self, policy: &WindowPolicy) -> Result<()>;
    /// Idempotent: set every flag of the policy (and re-assert level /
    /// collection behaviour / topmost), then read the capture flag back.
    fn apply(&self, policy: &WindowPolicy) -> Result<Readback>;
    /// Read-back only.
    fn verify(&self, policy: &WindowPolicy) -> Readback;
    /// Show without activating the app (unless `policy.activates`).
    fn show(&self, policy: &WindowPolicy) -> Result<()>;
    fn hide(&self);
    fn is_visible(&self) -> bool;
    fn set_interaction(&self, i: Interaction) -> Result<()>;
    /// Global top-left coordinates.
    fn frame(&self) -> Rect;
    fn set_origin(&self, x: f64, y: f64);
    /// Visible frames of all displays, global top-left coordinates.
    fn display_frames(&self) -> Vec<Rect>;
    /// True while the window is the key window / foreground (for blur detection).
    fn is_key(&self) -> bool;
}

pub fn unavailable(detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::StealthUnavailable).with_detail(detail)
}

/// Scriptable backend for unit tests (also used by integration tests of dependents).
pub mod fake {
    use super::*;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[derive(Default)]
    pub struct FakeState {
        pub excluded: bool,
        pub visible: bool,
        pub interaction: Option<Interaction>,
        pub origin: (f64, f64),
        pub size: (f64, f64),
        pub displays: Vec<Rect>,
        /// `apply` silently fails to set the flag (OS ignores us).
        pub refuse_flag: bool,
        /// Flag silently resets to false after the next `show`.
        pub reset_on_show: bool,
        pub fail_prepare: bool,
        pub calls: Vec<String>,
    }

    #[derive(Clone, Default)]
    pub struct FakeBackend(pub Arc<Mutex<FakeState>>);

    impl FakeBackend {
        pub fn new() -> Self {
            let b = Self::default();
            {
                let mut s = b.0.lock();
                s.size = (360.0, 160.0);
                s.displays = vec![Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1440.0,
                    h: 900.0,
                }];
            }
            b
        }
        pub fn calls(&self) -> Vec<String> {
            self.0.lock().calls.clone()
        }
        fn rb(s: &FakeState) -> Readback {
            Readback {
                capture_excluded: s.excluded,
                window_server_excluded: None,
                detail: format!("fake excluded={}", s.excluded),
            }
        }
    }

    impl WindowBackend for FakeBackend {
        fn prepare(&self, _p: &WindowPolicy) -> Result<()> {
            let mut s = self.0.lock();
            s.calls.push("prepare".into());
            if s.fail_prepare {
                return Err(unavailable("prepare failed"));
            }
            Ok(())
        }
        fn apply(&self, p: &WindowPolicy) -> Result<Readback> {
            let mut s = self.0.lock();
            s.calls.push("apply".into());
            if !s.refuse_flag {
                s.excluded = p.capture_excluded;
            }
            Ok(Self::rb(&s))
        }
        fn verify(&self, _p: &WindowPolicy) -> Readback {
            Self::rb(&self.0.lock())
        }
        fn show(&self, _p: &WindowPolicy) -> Result<()> {
            let mut s = self.0.lock();
            s.calls.push("show".into());
            s.visible = true;
            if s.reset_on_show {
                s.excluded = false;
                s.reset_on_show = false;
            }
            Ok(())
        }
        fn hide(&self) {
            let mut s = self.0.lock();
            s.calls.push("hide".into());
            s.visible = false;
        }
        fn is_visible(&self) -> bool {
            self.0.lock().visible
        }
        fn set_interaction(&self, i: Interaction) -> Result<()> {
            let mut s = self.0.lock();
            s.calls.push(format!("interaction:{i:?}"));
            s.interaction = Some(i);
            Ok(())
        }
        fn frame(&self) -> Rect {
            let s = self.0.lock();
            Rect {
                x: s.origin.0,
                y: s.origin.1,
                w: s.size.0,
                h: s.size.1,
            }
        }
        fn set_origin(&self, x: f64, y: f64) {
            self.0.lock().origin = (x, y);
        }
        fn display_frames(&self) -> Vec<Rect> {
            self.0.lock().displays.clone()
        }
        fn is_key(&self) -> bool {
            self.0.lock().interaction == Some(Interaction::Interactive)
        }
    }
}
