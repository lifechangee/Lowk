//! Echo cancellation glue (ARCHITECTURE 6.3). WebRTC AEC3 runs on the Me stream at 16 kHz
//! in 10 ms frames with the System stream as the far-end reference. The processor exists
//! only with the `aec` cargo feature; without it the link is inert and `enabled` is false.
use lowk_types::AecMode;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// Max reference backlog (ms of 16 kHz audio) before the oldest is dropped, so the far-end
/// signal never lags the mic by more than the delay AEC3's estimator can track.
const MAX_REF: usize = 16 * 400;

#[derive(Default)]
struct Inner {
    enabled: AtomicBool,
    /// Bumped by [`AecLink::restart`]; the Me worker rebuilds its processor when it changes.
    generation: AtomicU32,
    reference: Mutex<VecDeque<f32>>,
}

/// Shared by the Me worker (consumer) and the System worker (producer).
#[derive(Clone, Default)]
pub struct AecLink(Arc<Inner>);

impl AecLink {
    pub fn new(enabled: bool) -> Self {
        let l = Self::default();
        l.set_enabled(enabled);
        l
    }
    /// True when this build contains the WebRTC processor.
    pub const fn available() -> bool {
        cfg!(feature = "aec")
    }
    pub fn set_enabled(&self, on: bool) {
        let on = on && Self::available();
        if !on {
            self.0.reference.lock().unwrap().clear();
        }
        self.0.enabled.store(on, Ordering::Relaxed);
    }
    /// Drops the far-end backlog and makes the Me worker build a fresh processor (used when the
    /// microphone is reopened: the old adaptive filter belongs to a device that has been closed).
    pub fn restart(&self) {
        self.0.reference.lock().unwrap().clear();
        self.0.generation.fetch_add(1, Ordering::AcqRel);
    }
    pub fn generation(&self) -> u32 {
        self.0.generation.load(Ordering::Acquire)
    }
    pub fn enabled(&self) -> bool {
        self.0.enabled.load(Ordering::Relaxed)
    }
    /// Far-end (System) 16 kHz samples.
    pub fn feed_reference(&self, pcm: &[f32]) {
        if !self.enabled() {
            return;
        }
        let mut q = self.0.reference.lock().unwrap();
        q.extend(pcm.iter().copied());
        while q.len() > MAX_REF {
            q.pop_front();
        }
    }
    #[cfg(feature = "aec")]
    fn pop_reference(&self, out: &mut [f32]) {
        let mut q = self.0.reference.lock().unwrap();
        for o in out.iter_mut() {
            *o = q.pop_front().unwrap_or(0.0);
        }
    }
    /// The per-worker processor (`None` without the `aec` feature).
    pub fn processor(&self) -> Option<AecProc> {
        AecProc::new()
    }
}

/// Whether echo cancellation runs: it needs both streams, a microphone that is actually open, and
/// a mode that wants it (`headphones` only matters for `Auto`). With the mic off AEC is bypassed.
pub fn aec_should_run(mode: AecMode, headphones: bool, mic_open: bool, has_me: bool, has_sys: bool) -> bool {
    let on = match mode {
        AecMode::On => true,
        AecMode::Off => false,
        AecMode::Auto => !headphones,
    };
    on && mic_open && has_me && has_sys
}

/// What the capture supervisor does about the Me device on one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicAction {
    /// Mic is intentionally off (or not configured) and closed: nothing runs, no watchdog, no retry.
    Idle,
    /// Mic was switched on: open the device (Bluetooth policy applies in the opener).
    Open,
    /// Mic was switched off: release the device and flush AEC.
    Close,
    /// Mic is open: run the health checks (lost device, default input moved, silent mic).
    Watch,
}

pub fn mic_action(configured: bool, mic_on: bool, open: bool) -> MicAction {
    match (configured && mic_on, open) {
        (false, false) => MicAction::Idle,
        (false, true) => MicAction::Close,
        (true, false) => MicAction::Open,
        (true, true) => MicAction::Watch,
    }
}

#[cfg(feature = "aec")]
pub struct AecProc {
    p: webrtc_audio_processing::Processor,
}

#[cfg(feature = "aec")]
impl AecProc {
    fn new() -> Option<Self> {
        use webrtc_audio_processing::config::{Config, EchoCanceller};
        let p = webrtc_audio_processing::Processor::new(16_000).ok()?;
        p.set_config(Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Default::default()
        });
        Some(Self { p })
    }
    /// In place on one 20 ms Me frame; no-op while the link is disabled.
    pub fn process_frame(&mut self, pcm: &mut [f32; super::pipeline::FRAME], link: &AecLink) {
        if !link.enabled() {
            return;
        }
        let n = self.p.num_samples_per_frame(); // 160
        let mut far = vec![0f32; n];
        for chunk in pcm.chunks_mut(n) {
            if chunk.len() != n {
                continue;
            }
            link.pop_reference(&mut far);
            let _ = self.p.process_render_frame(std::iter::once(&mut far[..]));
            let _ = self.p.process_capture_frame(std::iter::once(&mut *chunk));
        }
    }
}

#[cfg(not(feature = "aec"))]
pub struct AecProc;

#[cfg(not(feature = "aec"))]
impl AecProc {
    fn new() -> Option<Self> {
        None
    }
    pub fn process_frame(&mut self, _pcm: &mut [f32; super::pipeline::FRAME], _link: &AecLink) {
        let _ = &_link.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aec_state_transitions_with_the_mic() {
        use AecMode::*;
        // mic open: runs per mode
        assert!(aec_should_run(On, true, true, true, true));
        assert!(aec_should_run(Auto, false, true, true, true));
        assert!(!aec_should_run(Auto, true, true, true, true));
        // mic off: bypassed in every mode
        for m in [On, Off, Auto] {
            assert!(!aec_should_run(m, false, false, true, true), "{m:?}");
        }
        // mic back on: runs again; no system stream: never
        assert!(aec_should_run(On, false, true, true, true));
        assert!(!aec_should_run(On, false, true, true, false));
    }

    #[test]
    fn restart_clears_reference_and_bumps_generation() {
        let l = AecLink::new(false);
        let g0 = l.generation();
        l.0.reference.lock().unwrap().extend([0.1f32; 160]);
        l.restart();
        assert!(l.0.reference.lock().unwrap().is_empty());
        assert_eq!(l.generation(), g0 + 1);
    }

    #[test]
    fn mic_action_matrix_keeps_the_watchdog_quiet_while_off() {
        use MicAction::*;
        assert_eq!(
            mic_action(true, false, false),
            Idle,
            "off and closed: no retry, no watchdog"
        );
        assert_eq!(mic_action(true, false, true), Close);
        assert_eq!(mic_action(true, true, false), Open);
        assert_eq!(mic_action(true, true, true), Watch);
        assert_eq!(mic_action(false, true, false), Idle, "no mic configured");
    }

    #[test]
    fn disabled_link_collects_nothing() {
        let l = AecLink::new(false);
        l.feed_reference(&[0.5; 320]);
        assert!(l.0.reference.lock().unwrap().is_empty());
    }

    /// Crash hunt (owner bug A): the processor must survive any mic/reference content a real
    /// call can produce: digital silence, full-scale clipping, DC, white noise, NaN/Inf from a
    /// misbehaving device, reference bursts larger than the backlog cap, reference starvation.
    #[cfg(feature = "aec")]
    #[test]
    fn fuzz_extreme_signals_never_panic_and_stay_finite() {
        let link = AecLink::new(true);
        let mut p = link.processor().unwrap();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32 * 2.0 - 1.0
        };
        for round in 0..6000usize {
            let kind = (round / 500) % 12;
            let mut mic = [0f32; 320];
            let mut far = [0f32; 320];
            for i in 0..320 {
                let (m, f) = match kind {
                    0 => (0.0, 0.0),
                    1 => (1.0, -1.0),
                    2 => (rnd() * 4.0, rnd() * 4.0),
                    3 => (rnd(), rnd()),
                    4 => (if i % 2 == 0 { 1.0 } else { -1.0 }, 0.7),
                    5 => (f32::NAN, 0.1),
                    6 => (0.1, f32::INFINITY),
                    7 => (f32::NEG_INFINITY, f32::NAN),
                    8 => (1.0e30, 1.0e30),
                    9 => (rnd() * 0.001, 0.0),
                    10 => (0.5, 0.5),
                    _ => ((i as f32 * 0.3).sin(), (i as f32 * 0.3).sin()),
                };
                mic[i] = m;
                far[i] = f;
            }
            match round % 7 {
                0 => {}
                1 => link.feed_reference(&[far.as_slice(), far.as_slice(), far.as_slice()].concat()),
                _ => link.feed_reference(&far),
            }
            p.process_frame(&mut mic, &link);
        }
        for f in 0..200 {
            let far: Vec<f32> = (0..320)
                .map(|i| ((f * 320 + i) as f32 * 0.07).sin() * 0.3)
                .collect();
            link.feed_reference(&far);
            let mut mic = [0f32; 320];
            for (i, m) in mic.iter_mut().enumerate() {
                *m = far[i] * 0.25;
            }
            p.process_frame(&mut mic, &link);
            assert!(mic.iter().all(|v| v.is_finite() && v.abs() <= 1.5), "frame {f}");
        }
    }

    #[cfg(feature = "aec")]
    #[test]
    fn echo_is_attenuated() {
        // far-end tone leaks into the mic at -12 dB; after convergence the residual must drop.
        let link = AecLink::new(true);
        let mut p = link.processor().unwrap();
        let tone = |i: usize| (i as f32 * 0.07).sin() * 0.3 + (i as f32 * 0.19).sin() * 0.2;
        let mut first = 0f32;
        let mut last = 0f32;
        let frames = 600;
        for f in 0..frames {
            let far: Vec<f32> = (0..320).map(|i| tone(f * 320 + i)).collect();
            link.feed_reference(&far);
            let mut mic = [0f32; 320];
            for (i, m) in mic.iter_mut().enumerate() {
                *m = far[i] * 0.25;
            }
            p.process_frame(&mut mic, &link);
            let e = crate::pipeline::rms(&mic);
            if f == 5 {
                first = e;
            }
            if f == frames - 1 {
                last = e;
            }
        }
        let input = 0.25 * 0.26; // rms of the leaked far-end tone
        assert!(last < input * 0.2, "input {input} first {first} last {last}");
    }
}
