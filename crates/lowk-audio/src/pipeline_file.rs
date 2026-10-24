//! `PipelineFileSource`: replays files as the *raw device* signals of the Me and System streams
//! (48 kHz native rate, optionally louder / clipped / with System leaking into the mic) through
//! the REAL capture pipeline: SPSC ring -> stream worker -> resampler -> AEC3 -> `Frame16k`.
//!
//! [`crate::FileAudioSource`] hands finished 16 kHz frames to the sink and therefore never
//! touches the ring, the resampler or the echo canceller; this one does, which makes it the
//! silent stand-in for "I spoke into the built-in mic while the speakers play the call"
//! (regression A). No sound is produced; no permissions are needed.
use crate::aec::AecLink;
use crate::file::{decode_mono, FileDone};
use crate::pipeline::{spawn_worker, Meter, RingIn, WorkerHandle};
use crate::{AudioBackend, AudioEventSink, CaptureConfig, CaptureHandle, FrameSink};
use lowk_types::{now_mono_ns, AecMode, AppError, AudioApp, DeviceInfo, ErrorCode, Result, Stream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Native rate the fake "device" delivers (a MacBook's built-in mic and speakers run at 48 kHz).
pub const DEVICE_RATE: u32 = 48_000;

#[derive(Clone, Debug)]
pub struct PipelineOpts {
    /// Linear gain on the mic signal (> 1 clips like a hot mic; samples are NOT clamped, a real
    /// HAL delivers floats outside [-1, 1] too).
    pub mic_gain: f32,
    /// Share of the System signal that leaks into the mic (speakers -> room -> microphone).
    pub echo_gain: f32,
    /// Loop the files until this many seconds of audio were played (0 = play each file once).
    pub seconds: f32,
    /// 1.0 = real time, 0.0 = as fast as the pipeline accepts audio.
    pub speed: f64,
}

impl Default for PipelineOpts {
    fn default() -> Self {
        Self {
            mic_gain: 1.0,
            echo_gain: 0.0,
            seconds: 0.0,
            speed: 1.0,
        }
    }
}

pub struct PipelineFileSource {
    me: Option<PathBuf>,
    system: Option<PathBuf>,
    opts: PipelineOpts,
    done: FileDone,
}

impl PipelineFileSource {
    pub fn new(me: Option<PathBuf>, system: Option<PathBuf>, opts: PipelineOpts) -> Self {
        Self {
            me,
            system,
            opts,
            done: FileDone::default(),
        }
    }
    pub fn done(&self) -> FileDone {
        self.done.clone()
    }
}

/// Linear interpolation to the device rate (test signal generator, not a product resampler).
fn to_device_rate(x: &[f32], rate: u32) -> Vec<f32> {
    if rate == DEVICE_RATE || x.is_empty() {
        return x.to_vec();
    }
    let n = (x.len() as u64 * DEVICE_RATE as u64 / rate as u64) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * rate as f64 / DEVICE_RATE as f64;
            let (k, f) = (p as usize, (p - p.floor()) as f32);
            let a = x[k.min(x.len() - 1)];
            let b = x[(k + 1).min(x.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

/// A device signal: `base` repeated up to `total` samples, produced chunk by chunk, so a
/// multi-hour soak never holds the whole signal in memory (only the short base clip).
struct LoopedPcm {
    base: Vec<f32>,
    total: usize,
}

impl LoopedPcm {
    fn new(base: Vec<f32>, secs: f32) -> Self {
        let total = if secs <= 0.0 || base.is_empty() {
            base.len()
        } else {
            (secs * DEVICE_RATE as f32) as usize
        };
        Self { base, total }
    }
    /// Samples `[at, at + n)` (clipped to `total`).
    fn chunk(&self, at: usize, n: usize, out: &mut Vec<f32>) {
        out.clear();
        let end = (at + n).min(self.total);
        out.extend((at..end).map(|i| self.base[i % self.base.len()]));
    }
    fn materialize(&self) -> Vec<f32> {
        (0..self.total).map(|i| self.base[i % self.base.len()]).collect()
    }
}

struct Handle {
    stop: Arc<AtomicBool>,
    joins: Vec<std::thread::JoinHandle<()>>,
    _workers: Vec<WorkerHandle>,
}
impl CaptureHandle for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for j in self.joins.drain(..) {
            let _ = j.join();
        }
    }
}

impl AudioBackend for PipelineFileSource {
    fn list_apps(&self) -> Result<Vec<AudioApp>> {
        Ok(vec![])
    }
    fn list_mics(&self) -> Result<Vec<DeviceInfo>> {
        Ok(vec![])
    }
    fn start(
        &mut self,
        cfg: &CaptureConfig,
        sink: FrameSink,
        ev: AudioEventSink,
    ) -> Result<Box<dyn CaptureHandle>> {
        let load = |p: &Option<PathBuf>| -> Result<Option<LoopedPcm>> {
            match p {
                Some(p) => {
                    let (m, r) = decode_mono(p)?;
                    Ok(Some(LoopedPcm::new(to_device_rate(&m, r), self.opts.seconds)))
                }
                None => Ok(None),
            }
        };
        let (me, sys) = (load(&self.me)?, load(&self.system)?);
        if me.is_none() && sys.is_none() {
            return Err(AppError::new(ErrorCode::FormatInvalid).with_detail("no input files"));
        }
        // the mic hears the room: its own signal plus a share of the speakers (this one is
        // materialized; the System signal alone streams lazily)
        let me = me.map(|m| {
            let mut v = m.materialize();
            for (i, x) in v.iter_mut().enumerate() {
                let leak = sys
                    .as_ref()
                    .filter(|s| i < s.total)
                    .map_or(0.0, |s| s.base[i % s.base.len()]);
                *x = *x * self.opts.mic_gain + leak * self.opts.echo_gain;
            }
            let total = v.len();
            LoopedPcm { base: v, total }
        });
        let aec = AecLink::new(cfg.aec != AecMode::Off && me.is_some() && sys.is_some());
        let mut workers = vec![];
        let mut rings: Vec<(LoopedPcm, RingIn)> = vec![];
        let (mut me_sh, mut sys_sh) = (None, None);
        for (st, pcm) in [(Stream::Me, me), (Stream::System, sys)] {
            let Some(pcm) = pcm else { continue };
            let w = spawn_worker(st, sink.clone(), ev.clone(), Some(aec.clone()));
            let ring = w.new_ring(DEVICE_RATE);
            match st {
                Stream::Me => me_sh = Some(w.shared.clone()),
                Stream::System => sys_sh = Some(w.shared.clone()),
            }
            rings.push((pcm, ring));
            workers.push(w);
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.done.add(rings.len() as u32);
        let mut joins = vec![];
        for (pcm, mut ring) in rings {
            let (stop, done, speed) = (stop.clone(), self.done.clone(), self.opts.speed);
            joins.push(
                std::thread::Builder::new()
                    .name("audio-pipe-file".into())
                    .spawn(move || {
                        let t0 = Instant::now();
                        const CHUNK: usize = 480; // 10 ms at 48 kHz, like a HAL buffer
                        let mut sent = 0usize;
                        let mut ch: Vec<f32> = Vec::with_capacity(CHUNK);
                        for i in 0..pcm.total.div_ceil(CHUNK) {
                            if stop.load(Ordering::Relaxed) {
                                break;
                            }
                            pcm.chunk(i * CHUNK, CHUNK, &mut ch);
                            if speed > 0.0 {
                                let due = t0 + Duration::from_secs_f64(i as f64 * 0.01 / speed);
                                let now = Instant::now();
                                if due > now {
                                    std::thread::sleep(due - now);
                                }
                            } else {
                                // unthrottled: never outrun the worker (the ring would drop audio)
                                while sent > 0 && ring_backlog_high(&ring) && !stop.load(Ordering::Relaxed) {
                                    std::thread::sleep(Duration::from_millis(1));
                                }
                            }
                            ring.push(&ch, DEVICE_RATE, now_mono_ns());
                            sent += ch.len();
                        }
                        done.finish_one();
                    })
                    .expect("spawn pipeline file source"),
            );
        }
        let (stop2, ev2) = (stop.clone(), ev);
        joins.push(
            std::thread::Builder::new()
                .name("audio-pipe-meter".into())
                .spawn(move || {
                    let mut m = Meter::new(me_sh, sys_sh, ev2);
                    while !stop2.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(100));
                        m.tick();
                    }
                })
                .expect("spawn meter"),
        );
        Ok(Box::new(Handle {
            stop,
            joins,
            _workers: workers,
        }))
    }
}

/// True when the ring holds more than ~1 s (the producer should wait).
fn ring_backlog_high(r: &RingIn) -> bool {
    r.backlog() > DEVICE_RATE as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::Frame16k;
    use std::sync::Mutex;

    fn write_wav(path: &std::path::Path, rate: u32, secs: f32, freq: f32, amp: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..(rate as f32 * secs) as usize {
            let s =
                ((2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * amp * 32767.0) as i16;
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lowk-pipe-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Regression A, silent: 60 s of Me + System through ring/resampler/AEC with a hot (clipping)
    /// mic and a strong echo must deliver every frame, finite, in order, without panics.
    /// (The AEC build runs the real AEC3; without the feature the same path runs with AEC inert.)
    #[test]
    fn sixty_seconds_hot_mic_with_echo_through_the_real_pipeline() {
        let d = dir("60s");
        let (me, sys) = (d.join("me.wav"), d.join("sys.wav"));
        write_wav(&me, 16_000, 7.0, 300.0, 0.6);
        write_wav(&sys, 44_100, 5.0, 700.0, 0.5);
        let mut src = PipelineFileSource::new(
            Some(me),
            Some(sys),
            PipelineOpts {
                mic_gain: 6.0,
                echo_gain: 0.8,
                seconds: 60.0,
                speed: 0.0,
            },
        );
        let done = src.done();
        let got: Arc<Mutex<Vec<Frame16k>>> = Arc::default();
        let g2 = got.clone();
        let cfg = CaptureConfig {
            system: Some(lowk_types::Target::WholeSystem),
            mic: None,
            aec: AecMode::On,
            mic_on: true,
        };
        let h = src
            .start(
                &cfg,
                Arc::new(move |f| g2.lock().unwrap().push(f)),
                Arc::new(|_| {}),
            )
            .unwrap();
        assert!(done.wait(Duration::from_secs(120)), "feeder finished");
        std::thread::sleep(Duration::from_millis(500));
        drop(h);
        let g = got.lock().unwrap();
        for st in [Stream::Me, Stream::System] {
            let f: Vec<&Frame16k> = g.iter().filter(|f| f.stream == st).collect();
            // 60 s = 3000 frames of 20 ms (the resampler delay eats a few)
            assert!(f.len() >= 2900, "{st:?}: {} frames", f.len());
            assert!(f
                .iter()
                .all(|f| f.pcm.len() == 320 && f.pcm.iter().all(|v| v.is_finite())));
            assert!(
                f.windows(2).all(|w| w[1].t_mono_ns > w[0].t_mono_ns),
                "{st:?} strictly increasing"
            );
        }
        std::fs::remove_dir_all(d).ok();
    }
}
