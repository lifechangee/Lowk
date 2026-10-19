//! Unsupported platforms (Linux CI): no hooks, degraded status only.
use super::{HookError, HookHandle, HookInfo, PermSnapshot};
use crate::engine::Engine;
use std::sync::Arc;

pub fn start(_: Arc<Engine>, _: bool) -> Result<(HookHandle, HookInfo), HookError> {
    Err(HookError::Unsupported)
}
pub fn permissions() -> PermSnapshot {
    PermSnapshot::default()
}
pub fn request_input_monitoring() -> bool {
    true
}
pub fn request_accessibility() -> bool {
    true
}
pub fn cursor_position() -> (f64, f64) {
    (0.0, 0.0)
}
