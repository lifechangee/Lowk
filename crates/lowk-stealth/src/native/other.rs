//! Unsupported platforms: nothing can claim to be protected.

use crate::backend::{unavailable, WindowBackend};
use crate::RawWindow;
use lowk_types::Result;
use std::sync::Arc;

pub fn make_backend(_w: &RawWindow) -> Result<Arc<dyn WindowBackend>> {
    Err(unavailable("stealth is only implemented for macOS and Windows"))
}

pub fn install_observers(_on_change: Arc<dyn Fn() + Send + Sync>) {}
