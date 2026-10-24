//! Platform independent half of the capture path:
//! `RT callback -> SPSC ring -> worker (rubato -> 20 ms frames [-> AEC on Me]) -> FrameSink`.
//!
//! * The RT side only calls [`RingIn::push`] (copy + atomics, no allocation, no locks).
//! * Each stream has one worker thread (`audio-me` / `audio-sys`). A rebuilt tap/mic simply
//!   hands the worker a fresh ring consumer ([`WorkerHandle::attach`]); the worker, not the
//!   backend, owns resampling state.
//! * Frame timestamps are on the process-wide monotonic origin ([`lowk_types::now_mono_ns`]),
//!   evenly spaced 20 ms apart and re-anchored whenever they drift > 30 ms from the
//!   capture time implied by the ring (after drops or a tap rebuild).
use crate::aec::AecLink;
use crate::{AudioEvent, AudioEventSink, FrameSink};
use audioadapter_buffers::direct::InterleavedSlice;
use lowk_types::{now_mono_ns, Frame16k, MonoNs, Stream};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use rubato::{Fft, FixedSync, Resampler};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub const OUT_RATE: u32 = 16_000;
/// Samples per 20 ms frame at 16 kHz.
pub const FRAME: usize = 320;
const FRAME_NS: u64 = 20_000_000;
/// Seconds of native-rate audio the ring can hold (worker stall tolerance).
const RING_SECONDS: usize = 4;

/// State shared between the RT producer, the worker and the supervisor.
#[derive(Default)]
pub struct StreamShared {
    /// Native sample rate of the data currently in the ring (0 = nothing yet).
    pub rate: AtomicU32,
    /// Monotonic ns at the end of the newest pushed sample.
    pub end_ns: AtomicU64,
    /// Samples dropped because the ring was full.
    pub dropped: AtomicU64,
    /// Consecutive ms of exactly-zero output (silent-tap detector).
    pub zero_ms: AtomicU64,
    /// Max frame RMS (f32 bits, >= 0 so bit order == value order) since the last [`take_level`].
    level_bits: AtomicU32,
    /// 20 ms frames delivered to the sink.
    pub frames: AtomicU64,
    /// Total non-silent frames (probe / permission detection).
    pub loud_frames: AtomicU64,
}

impl StreamShared {
    pub fn take_level(&self) -> f32 {
        f32::from_bits(self.level_bits.swap(0, Ordering::Relaxed))
    }
    fn note_level(&self, rms: f32) {
        self.level_bits.fetch_max(rms.to_bits(), Ordering::Relaxed);
    }
}

/// RT-side producer. `push` never allocates or blocks.
pub struct RingIn {
    prod: HeapProd<f32>,
    shared: Arc<StreamShared>,
}

impl RingIn {
    /// A fresh ring for `max_rate` Hz audio. The consumer goes to the worker.
    pub fn new(shared: Arc<StreamShared>, max_rate: u32) -> (Self, HeapCons<f32>) {
        let rb = HeapRb::<f32>::new(max_rate.max(16_000) as usize * RING_SECONDS);
        let (prod, cons) = rb.split();
        (Self { prod, shared }, cons)
    }

    /// Samples queued in the ring (producer side view).
    pub fn backlog(&self) -> usize {
        self.prod.occupied_len()
    }

    /// Mono f32 samples at `rate` Hz whose last sample was captured at `end_ns` (mono clock).
    pub fn push(&mut self, samples: &[f32], rate: u32, end_ns: MonoNs) {
        let n = self.prod.push_slice(samples);
        if n < samples.len() {
            self.shared
                .dropped
                .fetch_add((samples.len() - n) as u64, Ordering::Relaxed);
        }
        self.shared.end_ns.store(end_ns, Ordering::Release);
        self.shared.rate.store(rate, Ordering::Release);
    }
}

/// Mono -> 16 kHz converter producing exactly [`FRAME`] samples per call.
enum Conv {
    Pass,
    Fft(Box<Fft<f32>>),
}

impl Conv {
    fn new(rate: u32) -> Result<Self, String> {
        if rate == OUT_RATE {
            return Ok(Conv::Pass);
        }
        Fft::<f32>::new(rate as usize, OUT_RATE as usize, FRAME, 1, FixedSync::Output)
            .map(|f| Conv::Fft(Box::new(f)))
            .map_err(|e| format!("resampler {rate}->16000: {e}"))
    }
    /// Native frames needed for the next output frame.
    fn need(&self) -> usize {
        match self {
            Conv::Pass => FRAME,
            Conv::Fft(f) => f.input_frames_next(),
        }
    }
    fn run(&mut self, input: &[f32], out: &mut [f32; FRAME]) -> Result<(), String> {
        match self {
            Conv::Pass => {
                out.copy_from_slice(&input[..FRAME]);
                Ok(())
            }
            Conv::Fft(f) => {
                let a = InterleavedSlice::new(input, 1, input.len()).map_err(|e| e.to_string())?;
                let mut b = InterleavedSlice::new_mut(&mut out[..], 1, FRAME).map_err(|e| e.to_string())?;
                f.process_into_buffer(&a, &mut b, None)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
        }
    }
}

pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// RMS (linear, 0..1) -> UI meter 0..1 on a -60..0 dBFS scale.
pub fn level_to_meter(rms: f32) -> f32 {
    if rms <= 1e-6 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Handle to a stream worker. Dropping it stops the thread.
pub struct WorkerHandle {
    pub shared: Arc<StreamShared>,
    attach_tx: Sender<HeapCons<f32>>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    /// Replaces the ring the worker reads (after a rebuild). The old ring is dropped.
    pub fn attach(&self, cons: HeapCons<f32>) {
        let _ = self.attach_tx.send(cons);
    }
    /// For other threads that need to hand the worker fresh rings (Windows capture loops).
    pub fn attach_sender(&self) -> Sender<HeapCons<f32>> {
        self.attach_tx.clone()
    }
    /// A new ring wired to this worker.
    pub fn new_ring(&self, max_rate: u32) -> RingIn {
        let (p, c) = RingIn::new(self.shared.clone(), max_rate);
        self.attach(c);
        p
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Spawns the worker for one stream. `aec` is `Some` only for the Me stream (the
/// processor) or the System stream (reference feeder), see [`AecLink`].
pub fn spawn_worker(
    stream: Stream,
    sink: FrameSink,
    ev: AudioEventSink,
    aec: Option<AecLink>,
) -> WorkerHandle {
    let shared = Arc::new(StreamShared::default());
    let (attach_tx, attach_rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let name = match stream {
        Stream::Me => "audio-me",
        Stream::System => "audio-sys",
    };
    let (sh, st) = (shared.clone(), stop.clone());
    let join = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || worker_loop(stream, sh, st, attach_rx, sink, ev, aec))
        .expect("spawn audio worker");
    WorkerHandle {
        shared,
        attach_tx,
        stop,
        join: Some(join),
    }
}

fn worker_loop(
    stream: Stream,
    shared: Arc<StreamShared>,
    stop: Arc<AtomicBool>,
    attach_rx: Receiver<HeapCons<f32>>,
    sink: FrameSink,
    ev: AudioEventSink,
    aec: Option<AecLink>,
) {
    let mut cons: Option<HeapCons<f32>> = None;
    let mut cur_rate = 0u32;
    let mut conv = Conv::Pass;
    let mut inbuf: Vec<f32> = vec![0.0; 4096];
    let mut out = [0f32; FRAME];
    let mut t_next: Option<u64> = None;
    let mut last_t: Option<u64> = None;
    let mut proc = aec.as_ref().and_then(|l| l.processor());
    let mut aec_gen = aec.as_ref().map_or(0, |l| l.generation());
    let mut conv_failed = false;

    while !stop.load(Ordering::Relaxed) {
        while let Ok(c) = attach_rx.try_recv() {
            cons = Some(c);
            cur_rate = 0; // re-read the rate, restart timeline
            t_next = None;
        }
        let Some(c) = cons.as_mut() else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        let rate = shared.rate.load(Ordering::Acquire);
        if rate == 0 {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        if rate != cur_rate {
            if cur_rate != 0 {
                tracing::info!(
                    ?stream,
                    from = cur_rate,
                    to = rate,
                    "sample rate changed, resampler rebuilt"
                );
                // Samples queued at the old rate are meaningless now.
                c.clear();
            }
            match Conv::new(rate) {
                Ok(cv) => {
                    conv = cv;
                    conv_failed = false;
                }
                Err(e) => {
                    if !conv_failed {
                        tracing::error!(?stream, "{e}");
                        conv_failed = true;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            }
            cur_rate = rate;
            t_next = None;
        }
        let mut progressed = false;
        loop {
            let need = conv.need();
            if c.occupied_len() < need {
                break;
            }
            if inbuf.len() < need {
                inbuf.resize(need, 0.0);
            }
            let end_ns = shared.end_ns.load(Ordering::Acquire);
            let n = c.pop_slice(&mut inbuf[..need]);
            if n < need {
                break;
            }
            let remaining = c.occupied_len();
            if let Err(e) = conv.run(&inbuf[..need], &mut out) {
                tracing::error!(?stream, "resample: {e}");
                break;
            }
            // capture time of the first native sample of this block
            let behind_ns = ((remaining + need) as u64).saturating_mul(1_000_000_000) / cur_rate as u64;
            let est = end_ns.saturating_sub(behind_ns);
            let t = match t_next {
                Some(t) if t.abs_diff(est) <= 30_000_000 => t,
                _ => est,
            };
            // A frame cannot end in the future; keep the sequence strictly increasing.
            let t = t
                .min(now_mono_ns().saturating_sub(FRAME_NS))
                .max(last_t.map_or(0, |l| l + 1));
            last_t = Some(t);
            t_next = Some(t + FRAME_NS);
            progressed = true;

            let all_zero = out.iter().all(|v| *v == 0.0);
            if all_zero {
                shared.zero_ms.fetch_add(20, Ordering::Relaxed);
            } else {
                shared.zero_ms.store(0, Ordering::Relaxed);
                shared.loud_frames.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(l) = aec.as_ref() {
                if stream == Stream::Me && l.generation() != aec_gen {
                    // mic reopened: fresh AEC3 state
                    aec_gen = l.generation();
                    proc = l.processor();
                }
                match stream {
                    Stream::System => l.feed_reference(&out),
                    Stream::Me => {
                        if let Some(p) = proc.as_mut() {
                            p.process_frame(&mut out, l);
                        }
                    }
                }
            }
            shared.note_level(rms(&out));
            shared.frames.fetch_add(1, Ordering::Relaxed);
            sink(Frame16k {
                stream,
                t_mono_ns: t,
                pcm: out.to_vec(),
            });
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(3));
        }
    }
    let _ = ev;
}

/// 10 Hz `Level` + `Dropped` reporter. Call [`Meter::tick`] every 100 ms.
pub struct Meter {
    me: Option<Arc<StreamShared>>,
    sys: Option<Arc<StreamShared>>,
    ev: AudioEventSink,
    last_drop: [u64; 2],
    ticks: u32,
}

impl Meter {
    pub fn new(me: Option<Arc<StreamShared>>, sys: Option<Arc<StreamShared>>, ev: AudioEventSink) -> Self {
        Self {
            me,
            sys,
            ev,
            last_drop: [0; 2],
            ticks: 0,
        }
    }
    pub fn tick(&mut self) {
        let lv =
            |s: &Option<Arc<StreamShared>>| s.as_ref().map(|s| level_to_meter(s.take_level())).unwrap_or(0.0);
        (self.ev)(AudioEvent::Level {
            me: lv(&self.me),
            sys: lv(&self.sys),
        });
        self.ticks += 1;
        if self.ticks % 10 == 0 {
            for (i, (s, st)) in [(&self.me, Stream::Me), (&self.sys, Stream::System)]
                .into_iter()
                .enumerate()
            {
                if let Some(s) = s {
                    let d = s.dropped.load(Ordering::Relaxed);
                    if d > self.last_drop[i] {
                        (self.ev)(AudioEvent::Dropped {
                            stream: st,
                            samples: d - self.last_drop[i],
                        });
                        self.last_drop[i] = d;
                    }
                }
            }
        }
    }
}

/// Helper for tests and the file backend: native-rate mono audio -> 16 kHz frames, offline.
pub fn resample_all(input: &[f32], rate: u32) -> Result<Vec<f32>, String> {
    let mut conv = Conv::new(rate)?;
    let mut out = Vec::with_capacity(input.len() * OUT_RATE as usize / rate as usize + FRAME);
    let mut pos = 0;
    let mut frame = [0f32; FRAME];
    // Pad so the tail is flushed through the resampler delay.
    let mut src = input.to_vec();
    src.extend(std::iter::repeat_n(0.0, rate as usize / 20));
    loop {
        let need = conv.need();
        if pos + need > src.len() {
            break;
        }
        conv.run(&src[pos..pos + need], &mut frame)?;
        pos += need;
        out.extend_from_slice(&frame);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn sine(freq: f32, rate: u32, secs: f32, amp: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * amp)
            .collect()
    }

    pub fn goertzel(x: &[f32], freq: f64, sr: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq / sr;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0f64, 0f64);
        for v in x {
            let s = *v as f64 + c * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        let p = s1 * s1 + s2 * s2 - c * s1 * s2;
        p.max(0.0).sqrt() * 2.0 / x.len() as f64
    }

    #[test]
    fn meter_scale() {
        assert_eq!(level_to_meter(0.0), 0.0);
        assert!((level_to_meter(1.0) - 1.0).abs() < 1e-6);
        assert!((level_to_meter(0.001) - 0.0).abs() < 1e-3); // -60 dB
        assert!(level_to_meter(0.1) > 0.6);
    }

    #[test]
    fn resample_keeps_tone_and_frame_count() {
        for rate in [48_000u32, 44_100, 16_000, 24_000, 96_000] {
            let x = sine(440.0, rate, 2.0, 0.3);
            let y = resample_all(&x, rate).unwrap();
            assert_eq!(y.len() % FRAME, 0);
            let secs = y.len() as f32 / 16_000.0;
            assert!((secs - 2.0).abs() < 0.2, "rate {rate}: {secs}s");
            let mid = &y[8_000..24_000];
            let a = goertzel(mid, 440.0, 16_000.0);
            assert!(
                (a - 0.3 / 2f64.sqrt() * 2f64.sqrt()).abs() < 0.03,
                "rate {rate}: amp {a}"
            );
            assert!(goertzel(mid, 880.0, 16_000.0) < 0.01);
        }
    }

    #[test]
    fn worker_delivers_ordered_frames_with_levels_and_rate_change() {
        let got: Arc<Mutex<Vec<Frame16k>>> = Arc::default();
        let g2 = got.clone();
        let w = spawn_worker(
            Stream::System,
            Arc::new(move |f| g2.lock().unwrap().push(f)),
            Arc::new(|_| {}),
            None,
        );
        let mut ring = w.new_ring(48_000);
        let x = sine(440.0, 48_000, 1.0, 0.5);
        for ch in x.chunks(512) {
            ring.push(ch, 48_000, now_mono_ns());
            std::thread::sleep(Duration::from_micros(10_667));
        }
        let lvl = w.shared.take_level();
        assert!(lvl > 0.2, "level {lvl}");
        // switch to 44.1 kHz mid-stream
        let y = sine(440.0, 44_100, 0.5, 0.5);
        for ch in y.chunks(441) {
            ring.push(ch, 44_100, now_mono_ns());
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(100));
        drop(w);
        let fr = got.lock().unwrap();
        assert!(fr.len() >= 60, "{} frames", fr.len());
        assert!(fr
            .iter()
            .all(|f| f.pcm.len() == FRAME && f.stream == Stream::System));
        assert!(fr.windows(2).all(|p| p[1].t_mono_ns > p[0].t_mono_ns));
    }

    /// Device matrix (Bluetooth A2DP/HFP, built-in, USB): whatever rate the device really runs at
    /// (16/24/44.1/48/96 kHz), `seconds of 16 kHz out == seconds in`, with the tone intact. A
    /// session that declares a wrong rate (tap format 48 kHz on a 24 kHz aggregate) would fail this
    /// by 2x. Also a mid-stream rate flip (headset profile switch) stays ordered and keeps time.
    #[test]
    fn device_matrix_rates_keep_duration_and_pitch() {
        for rate in [16_000u32, 24_000, 32_000, 44_100, 48_000, 96_000] {
            let got: Arc<Mutex<Vec<Frame16k>>> = Arc::default();
            let g2 = got.clone();
            let w = spawn_worker(
                Stream::Me,
                Arc::new(move |f| g2.lock().unwrap().push(f)),
                Arc::new(|_| {}),
                None,
            );
            let mut ring = w.new_ring(rate);
            let x = sine(440.0, rate, 1.5, 0.4);
            let chunk = (rate / 100) as usize;
            for ch in x.chunks(chunk) {
                ring.push(ch, rate, now_mono_ns());
                std::thread::sleep(Duration::from_millis(2));
            }
            std::thread::sleep(Duration::from_millis(300));
            drop(w);
            let fr = got.lock().unwrap();
            let pcm: Vec<f32> = fr.iter().flat_map(|f| f.pcm.iter().copied()).collect();
            let secs = pcm.len() as f32 / 16_000.0;
            assert!(
                (secs - 1.5).abs() < 0.12,
                "rate {rate}: {secs}s of 16 kHz for 1.5 s in"
            );
            let mid = &pcm[8_000..16_000];
            assert!(goertzel(mid, 440.0, 16_000.0) > 0.3, "rate {rate}: pitch lost");
            assert!(fr.windows(2).all(|p| p[1].t_mono_ns > p[0].t_mono_ns));
        }
    }

    #[test]
    fn rate_flip_a2dp_to_hfp_and_back_keeps_time() {
        let got: Arc<Mutex<Vec<Frame16k>>> = Arc::default();
        let g2 = got.clone();
        let w = spawn_worker(
            Stream::Me,
            Arc::new(move |f| g2.lock().unwrap().push(f)),
            Arc::new(|_| {}),
            None,
        );
        let mut ring = w.new_ring(48_000);
        for rate in [48_000u32, 24_000, 16_000, 24_000, 48_000] {
            let x = sine(440.0, rate, 0.5, 0.4);
            for ch in x.chunks((rate / 100) as usize) {
                ring.push(ch, rate, now_mono_ns());
                std::thread::sleep(Duration::from_millis(2));
            }
            std::thread::sleep(Duration::from_millis(80));
        }
        std::thread::sleep(Duration::from_millis(300));
        drop(w);
        let fr = got.lock().unwrap();
        let secs = fr.len() as f32 * 0.02;
        // 5 x 0.5 s; each switch loses at most the queued tail (< 0.1 s)
        assert!(secs > 2.0 && secs < 2.7, "{secs}s");
        assert!(fr.windows(2).all(|p| p[1].t_mono_ns > p[0].t_mono_ns));
    }

    #[test]
    fn zero_run_counts_and_resets() {
        let w = spawn_worker(Stream::System, Arc::new(|_| {}), Arc::new(|_| {}), None);
        let mut ring = w.new_ring(48_000);
        for _ in 0..40 {
            ring.push(&[0.0; 480], 48_000, now_mono_ns());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(w.shared.zero_ms.load(Ordering::Relaxed) >= 200);
        ring.push(&[0.5; 4800], 48_000, now_mono_ns());
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(w.shared.zero_ms.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn full_ring_counts_drops() {
        let sh = Arc::new(StreamShared::default());
        let (mut p, _c) = RingIn::new(sh.clone(), 16_000);
        p.push(&vec![0.1; 16_000 * RING_SECONDS + 100], 16_000, 1);
        assert_eq!(sh.dropped.load(Ordering::Relaxed), 100);
    }
}
