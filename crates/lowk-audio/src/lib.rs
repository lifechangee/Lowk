//! Audio capture: per-OS backends, rings, resampling, AEC. M0 stub: trait only
//! plus a no-op backend. The real backends are seeded from `spikes/audio`.
use lowk_types::{AecMode, AppError, AudioApp, DeviceInfo, ErrorCode, Frame16k, Result, Stream, Target};
use std::sync::{Arc, Mutex};

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

/// Shared state behind [`FakeAudioBackend`]: lets tests push frames and events
/// into whatever sinks the session registered.
#[derive(Default)]
struct FakeShared {
    sinks: Option<(FrameSink, AudioEventSink)>,
    starts: u32,
    stops: u32,
}

/// Cloneable remote control for a [`FakeAudioBackend`].
#[derive(Clone, Default)]
pub struct FakeAudioRemote(Arc<Mutex<FakeShared>>);

impl FakeAudioRemote {
    /// Delivers a frame to the registered sink; false when not capturing.
    pub fn push_frame(&self, f: Frame16k) -> bool {
        let sink = self.0.lock().unwrap().sinks.as_ref().map(|s| s.0.clone());
        sink.map(|s| s(f)).is_some()
    }
    /// Delivers an audio event to the registered sink; false when not capturing.
    pub fn push_event(&self, e: AudioEvent) -> bool {
        let sink = self.0.lock().unwrap().sinks.as_ref().map(|s| s.1.clone());
        sink.map(|s| s(e)).is_some()
    }
    pub fn is_capturing(&self) -> bool {
        self.0.lock().unwrap().sinks.is_some()
    }
    pub fn starts(&self) -> u32 {
        self.0.lock().unwrap().starts
    }
    pub fn stops(&self) -> u32 {
        self.0.lock().unwrap().stops
    }
}

struct FakeHandle(Arc<Mutex<FakeShared>>);
impl CaptureHandle for FakeHandle {}
impl Drop for FakeHandle {
    fn drop(&mut self) {
        let mut g = self.0.lock().unwrap();
        g.sinks = None;
        g.stops += 1;
    }
}

/// Test backend: lists one fake app and one fake mic; frames and events are
/// injected through [`FakeAudioRemote`].
#[derive(Default)]
pub struct FakeAudioBackend {
    remote: FakeAudioRemote,
}
impl FakeAudioBackend {
    pub fn new() -> (Self, FakeAudioRemote) {
        let b = Self::default();
        let r = b.remote.clone();
        (b, r)
    }
    pub fn remote(&self) -> FakeAudioRemote {
        self.remote.clone()
    }
}
impl AudioBackend for FakeAudioBackend {
    fn list_apps(&self) -> Result<Vec<AudioApp>> {
        Ok(vec![AudioApp {
            id: "fake.meet".into(),
            name: "Fake Meet".into(),
            producing_audio: true,
            pids: vec![1],
        }])
    }
    fn list_mics(&self) -> Result<Vec<DeviceInfo>> {
        Ok(vec![DeviceInfo {
            id: "fake-mic".into(),
            name: "Fake microphone".into(),
            is_default: true,
        }])
    }
    fn start(
        &mut self,
        _c: &CaptureConfig,
        s: FrameSink,
        e: AudioEventSink,
    ) -> Result<Box<dyn CaptureHandle>> {
        let mut g = self.remote.0.lock().unwrap();
        if g.sinks.is_some() {
            return Err(AppError::new(ErrorCode::SessionBusy));
        }
        g.sinks = Some((s, e));
        g.starts += 1;
        Ok(Box::new(FakeHandle(self.remote.0.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::Stream;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn fake_backend_routes_frames_until_handle_dropped() {
        let (mut b, remote) = FakeAudioBackend::new();
        let n = Arc::new(AtomicU32::new(0));
        let n2 = n.clone();
        let cfg = CaptureConfig {
            system: None,
            mic: None,
            aec: AecMode::Off,
        };
        let h = b
            .start(
                &cfg,
                Arc::new(move |_| {
                    n2.fetch_add(1, Ordering::SeqCst);
                }),
                Arc::new(|_| {}),
            )
            .unwrap();
        let f = Frame16k {
            stream: Stream::Me,
            t_mono_ns: 0,
            pcm: vec![0.0; 320],
        };
        assert!(remote.push_frame(f.clone()));
        assert!(remote.is_capturing());
        drop(h);
        assert!(!remote.push_frame(f));
        assert_eq!(
            (n.load(Ordering::SeqCst), remote.starts(), remote.stops()),
            (1, 1, 1)
        );
    }
}
