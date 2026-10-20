//! Screen sampling for the adaptive tone and the stealth self-test.
//! macOS: ScreenCaptureKit one-shots (gated on Screen Recording so the sampler
//! never triggers the system prompt). Windows: stub for M0 (GDI sampler later).
use lowk_types::{PermKind, PermState, Rect, RegionSampler, Result, ScreenGrab};
use std::sync::Arc;

#[cfg(target_os = "macos")]
fn granted() -> bool {
    lowk_platform::perm::status(PermKind::ScreenRecording) == PermState::Granted
}

#[cfg(target_os = "macos")]
struct Gated;

#[cfg(target_os = "macos")]
impl RegionSampler for Gated {
    fn grab_region(&self, rect: Rect, w: u32, h: u32) -> Result<Vec<u8>> {
        if !granted() {
            return Err(lowk_types::AppError::new(
                lowk_types::ErrorCode::PermScreenRecording,
            ));
        }
        lowk_capture::SckSampler.grab_region(rect, w, h)
    }
}

#[cfg(target_os = "macos")]
impl ScreenGrab for Gated {
    fn grab(&self, rect: Rect) -> Result<(Vec<u8>, u32, u32)> {
        if !granted() {
            return Err(lowk_types::AppError::new(
                lowk_types::ErrorCode::PermScreenRecording,
            ));
        }
        lowk_capture::SckGrab.grab(rect)
    }
}

#[cfg(target_os = "macos")]
pub fn sampler() -> Arc<dyn RegionSampler> {
    Arc::new(Gated)
}
#[cfg(target_os = "macos")]
pub fn screen_grab() -> Option<Arc<dyn ScreenGrab>> {
    Some(Arc::new(Gated))
}

#[cfg(not(target_os = "macos"))]
struct Unsupported;
#[cfg(not(target_os = "macos"))]
impl RegionSampler for Unsupported {
    fn grab_region(&self, _r: Rect, _w: u32, _h: u32) -> Result<Vec<u8>> {
        // M0: the tone stays at its last / default value until the GDI sampler lands.
        let _ = (PermKind::ScreenRecording, PermState::Granted);
        Err(lowk_types::AppError::new(lowk_types::ErrorCode::Unsupported))
    }
}
#[cfg(not(target_os = "macos"))]
pub fn sampler() -> Arc<dyn RegionSampler> {
    Arc::new(Unsupported)
}
#[cfg(not(target_os = "macos"))]
pub fn screen_grab() -> Option<Arc<dyn ScreenGrab>> {
    None
}
