//! `FileAudioSource`: replays audio files (WAV, m4a/AAC, mp3) as the Me / System streams at
//! real-time speed, through the same `Frame16k` contract as the live backends. Used by
//! tests, the QA harness and the bake-off tooling.
use crate::pipeline::{level_to_meter, resample_all, rms, FRAME};
use crate::{AudioBackend, AudioEvent, AudioEventSink, CaptureConfig, CaptureHandle, FrameSink};
use lowk_types::{now_mono_ns, AppError, AudioApp, DeviceInfo, ErrorCode, Frame16k, Result, Stream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Signals when every file has been played out completely.
#[derive(Clone, Default)]
pub struct FileDone(Arc<(Mutex<u32>, Condvar)>);

impl FileDone {
    pub(crate) fn add(&self, n: u32) {
        *self.0 .0.lock().unwrap() += n;
    }
    pub(crate) fn finish_one(&self) {
        let mut g = self.0 .0.lock().unwrap();
        *g = g.saturating_sub(1);
        self.0 .1.notify_all();
    }
    /// True when all playback threads of the last `start` have finished.
    pub fn is_done(&self) -> bool {
        *self.0 .0.lock().unwrap() == 0
    }
    /// Waits until done or `timeout`; returns `is_done()`.
    pub fn wait(&self, timeout: Duration) -> bool {
        let g = self.0 .0.lock().unwrap();
        let (g, _) = self.0 .1.wait_timeout_while(g, timeout, |n| *n > 0).unwrap();
        *g == 0
    }
}

pub struct FileAudioSource {
    me: Option<PathBuf>,
    system: Option<PathBuf>,
    /// 1.0 = real time, 0.0 = as fast as possible.
    speed: f64,
    done: FileDone,
}

impl FileAudioSource {
    pub fn new(me: Option<PathBuf>, system: Option<PathBuf>) -> Self {
        Self {
            me,
            system,
            speed: 1.0,
            done: FileDone::default(),
        }
    }
    /// Playback speed multiplier (0 = unthrottled, for unit tests).
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }
    pub fn done(&self) -> FileDone {
        self.done.clone()
    }
}

fn fmt_err(msg: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::FormatInvalid).with_detail(msg.to_string())
}

/// Decodes any supported file to mono f32 at its native rate.
pub fn decode_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::errors::Error as SErr;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path)
        .map_err(|e| AppError::new(ErrorCode::Io).with_detail(format!("{}: {e}", path.display())))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(fmt_err)?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| fmt_err("no audio track"))?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &Default::default())
        .map_err(fmt_err)?;
    let mut rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut mono: Vec<f32> = vec![];
    let mut sbuf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SErr::ResetRequired) => break,
            Err(e) => return Err(fmt_err(e)),
        };
        if packet.track_id() != track.id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SErr::DecodeError(_)) => continue,
            Err(e) => return Err(fmt_err(e)),
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        let ch = spec.channels.count().max(1);
        let sb = sbuf.get_or_insert_with(|| SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        if sb.capacity() < decoded.capacity() * ch {
            *sb = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        }
        sb.copy_interleaved_ref(decoded);
        for fr in sb.samples().chunks_exact(ch) {
            mono.push(fr.iter().sum::<f32>() / ch as f32);
        }
    }
    if rate == 0 || mono.is_empty() {
        return Err(fmt_err("empty or undecodable audio"));
    }
    Ok((mono, rate))
}

/// Decodes any supported file to 16 kHz mono f32 (whole file).
pub fn decode_16k(path: &Path) -> Result<Vec<f32>> {
    let (m, r) = decode_mono(path)?;
    resample_all(&m, r).map_err(fmt_err)
}

struct Handle {
    stop: Arc<AtomicBool>,
    joins: Vec<std::thread::JoinHandle<()>>,
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

impl AudioBackend for FileAudioSource {
    fn list_apps(&self) -> Result<Vec<AudioApp>> {
        Ok(vec![])
    }
    fn list_mics(&self) -> Result<Vec<DeviceInfo>> {
        Ok(vec![])
    }
    fn start(
        &mut self,
        _cfg: &CaptureConfig,
        sink: FrameSink,
        ev: AudioEventSink,
    ) -> Result<Box<dyn CaptureHandle>> {
        let mut tracks: Vec<(Stream, Vec<f32>)> = vec![];
        for (st, p) in [(Stream::Me, &self.me), (Stream::System, &self.system)] {
            if let Some(p) = p {
                tracks.push((st, decode_16k(p)?));
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let levels = Arc::new(Mutex::new([0f32; 2]));
        self.done.add(tracks.len() as u32);
        let mut joins = vec![];
        for (st, pcm) in tracks {
            let (sink, stop, done, levels, speed) = (
                sink.clone(),
                stop.clone(),
                self.done.clone(),
                levels.clone(),
                self.speed,
            );
            joins.push(
                std::thread::Builder::new()
                    .name("audio-file".into())
                    .spawn(move || {
                        let t0 = Instant::now();
                        let idx = if st == Stream::Me { 0 } else { 1 };
                        for (i, ch) in pcm.chunks(FRAME).enumerate() {
                            if stop.load(Ordering::Relaxed) {
                                break;
                            }
                            if speed > 0.0 {
                                let due = t0 + Duration::from_secs_f64(i as f64 * 0.02 / speed);
                                let now = Instant::now();
                                if due > now {
                                    std::thread::sleep(due - now);
                                }
                            }
                            let mut f = ch.to_vec();
                            f.resize(FRAME, 0.0);
                            {
                                let mut l = levels.lock().unwrap();
                                l[idx] = l[idx].max(rms(&f));
                            }
                            sink(Frame16k {
                                stream: st,
                                t_mono_ns: now_mono_ns(),
                                pcm: f,
                            });
                        }
                        done.finish_one();
                    })
                    .expect("spawn file source"),
            );
        }
        // 10 Hz levels until stopped.
        let (stop2, levels2) = (stop.clone(), levels);
        joins.push(
            std::thread::Builder::new()
                .name("audio-file-meter".into())
                .spawn(move || {
                    while !stop2.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(100));
                        let mut l = levels2.lock().unwrap();
                        ev(AudioEvent::Level {
                            me: level_to_meter(l[0]),
                            sys: level_to_meter(l[1]),
                        });
                        *l = [0.0; 2];
                    }
                })
                .expect("spawn meter"),
        );
        Ok(Box::new(Handle { stop, joins }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AecMode;

    fn write_wav(path: &Path, rate: u32, ch: u16, secs: f32, freq: f32) {
        let spec = hound::WavSpec {
            channels: ch,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..(rate as f32 * secs) as usize {
            let s =
                ((2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.4 * 32767.0) as i16;
            for _ in 0..ch {
                w.write_sample(s).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    fn cfg() -> CaptureConfig {
        CaptureConfig {
            system: None,
            mic: None,
            aec: AecMode::Off,
            mic_on: true,
        }
    }

    #[test]
    fn decodes_stereo_44k_to_16k_mono() {
        let d = tempfile_dir();
        let p = d.join("a.wav");
        write_wav(&p, 44_100, 2, 1.0, 440.0);
        let x = decode_16k(&p).unwrap();
        assert!((x.len() as f32 / 16_000.0 - 1.0).abs() < 0.1);
        assert!(rms(&x[4_000..12_000]) > 0.2);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn replays_both_streams_in_real_time() {
        let d = tempfile_dir();
        let (a, b) = (d.join("me.wav"), d.join("sys.wav"));
        write_wav(&a, 16_000, 1, 0.5, 300.0);
        write_wav(&b, 48_000, 1, 0.5, 700.0);
        let mut src = FileAudioSource::new(Some(a), Some(b));
        let done = src.done();
        let got: Arc<Mutex<Vec<Frame16k>>> = Arc::default();
        let g2 = got.clone();
        let t = Instant::now();
        let h = src
            .start(
                &cfg(),
                Arc::new(move |f| g2.lock().unwrap().push(f)),
                Arc::new(|_| {}),
            )
            .unwrap();
        assert!(done.wait(Duration::from_secs(5)));
        let el = t.elapsed().as_secs_f32();
        assert!((0.4..1.2).contains(&el), "real time pacing: {el}s");
        drop(h);
        let g = got.lock().unwrap();
        for st in [Stream::Me, Stream::System] {
            let n = g.iter().filter(|f| f.stream == st).count();
            assert!((24..=27).contains(&n), "{st:?}: {n} frames");
        }
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn missing_or_garbage_file_is_an_error() {
        let d = tempfile_dir();
        let bad = d.join("x.wav");
        std::fs::write(&bad, b"not audio").unwrap();
        for p in [bad, d.join("nope.wav")] {
            let mut s = FileAudioSource::new(Some(p), None);
            assert!(s.start(&cfg(), Arc::new(|_| {}), Arc::new(|_| {})).is_err());
        }
        std::fs::remove_dir_all(d).ok();
    }

    fn tempfile_dir() -> PathBuf {
        use std::sync::atomic::AtomicU32;
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "lowk-audio-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
