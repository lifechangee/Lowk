//! Screenshots and region grabs. M0 stub (M3 owns it).
use lowk_types::{AppError, Rect, RegionSampler, Result};

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::{SckGrab, SckSampler};

#[derive(Debug, Clone)]
pub struct MonitorShot {
    pub index: u32,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
    /// Encoded JPEG/PNG bytes.
    pub data: Vec<u8>,
}

pub trait Screenshots: Send + Sync {
    fn screenshot_all(&self) -> Result<Vec<MonitorShot>>;
    fn screenshot_area(&self, rect: Rect) -> Result<MonitorShot>;
}

#[derive(Default)]
pub struct NoopCapture;
impl Screenshots for NoopCapture {
    fn screenshot_all(&self) -> Result<Vec<MonitorShot>> {
        Ok(vec![])
    }
    fn screenshot_area(&self, _rect: Rect) -> Result<MonitorShot> {
        Err(AppError::not_implemented())
    }
}
impl RegionSampler for NoopCapture {
    fn grab_region(&self, _rect: Rect, w: u32, h: u32) -> Result<Vec<u8>> {
        Ok(vec![0; (w * h * 4) as usize])
    }
}
