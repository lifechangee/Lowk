//! Unsupported platforms: nothing can claim to be protected.

use crate::backend::{unavailable, WindowBackend};
use crate::RawWindow;
use lowk_types::{CursorSource, Result};
use std::sync::Arc;

pub fn make_backend(_w: &RawWindow) -> Result<Arc<dyn WindowBackend>> {
    Err(unavailable("stealth is only implemented for macOS and Windows"))
}

struct NoCursor;
impl CursorSource for NoCursor {
    fn position(&self) -> (f64, f64) {
        (0.0, 0.0)
    }
}

pub fn cursor() -> Arc<dyn CursorSource> {
    Arc::new(NoCursor)
}

pub fn install_observers(_on_change: Arc<dyn Fn() + Send + Sync>) {}
