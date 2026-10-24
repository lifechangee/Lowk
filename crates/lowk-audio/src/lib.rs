//! Audio capture: per-OS backends, rings, resampling, AEC, file replay.
//!
//! * macOS: Core Audio process taps + HAL input ([`mac`]).
//! * Windows: WASAPI process loopback + capture ([`win`]).
//! * [`FileAudioSource`]: replays WAV/m4a files in real time (tests, QA harness).
//! * [`FakeAudioBackend`] / [`NoopAudioBackend`]: for dependants' tests.
//!
//! Everything funnels through [`pipeline`]: RT callback -> SPSC ring -> per-stream worker
//! (rubato -> 20 ms `Frame16k`, AEC on Me) -> the session's `FrameSink`.
pub mod aec;
pub mod file;
pub mod pipeline;
pub mod pipeline_file;
mod proc_tree;

#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(windows)]
pub mod win;

pub use aec::AecLink;
pub use file::{FileAudioSource, FileDone};
pub use lowk_platform::perm::SystemAudioProbe;
pub use lowk_types::{SourceInfo, SourceKind};
pub use pipeline_file::{PipelineFileSource, PipelineOpts};

/// `CaptureConfig::mic` value that disables the Me stream (None = default input device).
pub const MIC_NONE: &str = "none";

use lowk_types::{AecMode, AppError, AudioApp, DeviceInfo, ErrorCode, Frame16k, Result, Stream, Target};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub system: Option<Target>,
    pub mic: Option<String>,
    pub aec: AecMode,
    /// Whether the microphone is switched on right now. While false the Me device is NOT opened
    /// (no OS mic-in-use indicator); [`CaptureHandle::set_mic`] opens/closes it later.
    pub mic_on: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioEvent {
    Level {
        me: f32,
        sys: f32,
    },
    Dropped {
        stream: Stream,
        samples: u64,
    },
    TapRebuilt {
        reason: String,
    },
    DeviceChanged,
    SilentTap,
    TargetLost,
    /// The target came back after `TargetLost` (capture resumed by itself).
    TargetRestored,
    Permission(lowk_types::PermKind),
}

pub type FrameSink = Arc<dyn Fn(Frame16k) + Send + Sync>;
pub type AudioEventSink = Arc<dyn Fn(AudioEvent) + Send + Sync>;

/// Stops capture when dropped.
pub trait CaptureHandle: Send {
    /// Opens (`true`) or fully releases (`false`) the microphone device while the system stream keeps
    /// running. Backends without a real microphone keep the default no-op.
    fn set_mic(&self, _on: bool) -> Result<()> {
        Ok(())
    }
}

/// Implemented by `mac::CoreAudio`, `win::Wasapi`, [`FileAudioSource`] and the fakes.
/// `CaptureConfig::mic`: `None` = default input, `Some(MIC_NONE)` = no Me stream, else a
/// device id from `list_mics`. `CaptureConfig::system`: `None` = no System stream.
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

/// The real capture backend of this OS (a no-op elsewhere).
pub fn default_backend() -> Box<dyn AudioBackend> {
    #[cfg(target_os = "macos")]
    return Box::new(mac::CoreAudio);
    #[cfg(windows)]
    return Box::new(win::Wasapi);
    #[allow(unreachable_code)]
    Box::new(NoopAudioBackend)
}

/// Source picker, part 1: the whole system plus apps that have an audio process right now
/// (producing ones first). Cheap enough to poll at 1 Hz.
pub fn list_sources() -> Result<Vec<SourceInfo>> {
    #[cfg(target_os = "macos")]
    return mac::procs::list_sources();
    #[cfg(windows)]
    return win::list_sources();
    #[allow(unreachable_code)]
    Ok(vec![])
}

/// Source picker, part 2: installed apps (so a call app can be chosen before it plays).
pub fn list_installed_sources() -> Result<Vec<SourceInfo>> {
    #[cfg(target_os = "macos")]
    return mac::procs::list_installed();
    #[cfg(windows)]
    return win::list_installed();
    #[allow(unreachable_code)]
    Ok(vec![])
}

/// G4: triggers the real macOS "System Audio Recording" prompt by tapping a quiet tone we
/// play ourselves, and reports whether audio actually arrived. Blocks up to `timeout`
/// (the user may be answering the prompt). Other OSes have no such permission.
pub fn probe_system_audio(timeout: std::time::Duration) -> SystemAudioProbe {
    #[cfg(target_os = "macos")]
    return mac::probe_system_audio(timeout);
    #[allow(unreachable_code)]
    {
        let _ = timeout;
        SystemAudioProbe::Working
    }
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
    /// Fake microphone source: currently open, and how many times it was opened / closed.
    mic_open: bool,
    mic_opens: u32,
    mic_closes: u32,
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
    /// True while the fake microphone source is open (the device a real backend would hold).
    pub fn mic_open(&self) -> bool {
        self.0.lock().unwrap().mic_open
    }
    pub fn mic_opens(&self) -> u32 {
        self.0.lock().unwrap().mic_opens
    }
    pub fn mic_closes(&self) -> u32 {
        self.0.lock().unwrap().mic_closes
    }
}

struct FakeHandle(Arc<Mutex<FakeShared>>);
impl CaptureHandle for FakeHandle {
    fn set_mic(&self, on: bool) -> Result<()> {
        let mut g = self.0.lock().unwrap();
        match (on, g.mic_open) {
            (true, false) => {
                g.mic_open = true;
                g.mic_opens += 1;
            }
            (false, true) => {
                g.mic_open = false;
                g.mic_closes += 1;
            }
            _ => {}
        }
        Ok(())
    }
}
impl Drop for FakeHandle {
    fn drop(&mut self) {
        let mut g = self.0.lock().unwrap();
        g.sinks = None;
        g.stops += 1;
        if g.mic_open {
            g.mic_open = false;
            g.mic_closes += 1;
        }
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
        c: &CaptureConfig,
        s: FrameSink,
        e: AudioEventSink,
    ) -> Result<Box<dyn CaptureHandle>> {
        let mut g = self.remote.0.lock().unwrap();
        if g.sinks.is_some() {
            return Err(AppError::new(ErrorCode::SessionBusy));
        }
        g.sinks = Some((s, e));
        g.starts += 1;
        if c.mic_on && c.mic.as_deref() != Some(MIC_NONE) {
            g.mic_open = true;
            g.mic_opens += 1;
        }
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
            mic_on: true,
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
