//! Audio capture: per-OS backends, rings, resampling, AEC. M0 stub: trait only
//! plus a no-op backend. The real backends are seeded from `spikes/audio`.
use lowk_types::{AecMode, AppError, AudioApp, DeviceInfo, Frame16k, Result, Stream, Target};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub system: Option<Target>,
    pub mic: Option<String>,
    pub aec: AecMode,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioEvent {
    Level { me: f32, sys: f32 },
    Dropped { stream: Stream, samples: u64 },
    TapRebuilt { reason: String },
    DeviceChanged,
    SilentTap,
    TargetLost,
    Permission(lowk_types::PermKind),
}

pub type FrameSink = Arc<dyn Fn(Frame16k) + Send + Sync>;
pub type AudioEventSink = Arc<dyn Fn(AudioEvent) + Send + Sync>;

/// Stops capture when dropped.
pub trait CaptureHandle: Send {}

/// Implemented by `mac::CoreAudio` and `win::Wasapi` (WP7+/M1).
pub trait AudioBackend: Send {
    fn list_apps(&self) -> Result<Vec<AudioApp>>;
    fn list_mics(&self) -> Result<Vec<DeviceInfo>>;
    fn start(
        &mut self,
        cfg: &CaptureConfig,
        sink: FrameSink,
        ev: AudioEventSink,
    ) -> Result<Box<dyn CaptureHandle>>;
}

/// Backend that lists nothing and captures nothing.
#[derive(Default)]
pub struct NoopAudioBackend;

struct NoopHandle;
impl CaptureHandle for NoopHandle {}

impl AudioBackend for NoopAudioBackend {
    fn list_apps(&self) -> Result<Vec<AudioApp>> {
        Ok(vec![])
    }
    fn list_mics(&self) -> Result<Vec<DeviceInfo>> {
        Ok(vec![])
    }
    fn start(
        &mut self,
        _c: &CaptureConfig,
        _s: FrameSink,
        _e: AudioEventSink,
    ) -> Result<Box<dyn CaptureHandle>> {
        Ok(Box::new(NoopHandle))
    }
}

/// Placeholder for a scripted-frame backend (tests); the owner fleshes it out.
#[derive(Default)]
pub struct FakeAudioBackend;
impl AudioBackend for FakeAudioBackend {
    fn list_apps(&self) -> Result<Vec<AudioApp>> {
        Err(AppError::not_implemented())
    }
    fn list_mics(&self) -> Result<Vec<DeviceInfo>> {
        Ok(vec![])
    }
    fn start(
        &mut self,
        _c: &CaptureConfig,
        _s: FrameSink,
        _e: AudioEventSink,
    ) -> Result<Box<dyn CaptureHandle>> {
        Ok(Box::new(NoopHandle))
    }
}
