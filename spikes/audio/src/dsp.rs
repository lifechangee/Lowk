//! Platform-independent pipeline: mono f32 chunks at native rate -> 16 kHz mono WAV,
//! with RMS + latency logging and zero-buffer (silent tap) detection.
use anyhow::Result;
use rubato::{FftFixedIn, Resampler};
use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const OUT_RATE: u32 = 16_000;
const CHUNK_IN: usize = 1024;

/// One IOProc delivery, mixed down to mono.
pub struct Chunk {
    pub samples: Vec<f32>,
    pub sr: f64,
    /// capture time of first sample, monotonic ns (same clock as `deliver_ns`)
    pub cap_ns: u64,
    /// time the callback ran, monotonic ns
    pub deliver_ns: u64,
}

#[derive(Default)]
pub struct Shared {
    /// consecutive ms of exactly-zero input observed (updated by worker)
    pub zero_ms: AtomicU64,
    pub total_in_frames: AtomicU64,
}

pub fn rms_dbfs(x: &[f32]) -> f32 {
    if x.is_empty() { return -120.0; }
    let s: f64 = x.iter().map(|v| (*v as f64) * (*v as f64)).sum();
    let r = (s / x.len() as f64).sqrt();
    (20.0 * r.max(1e-6).log10()) as f32
}

fn pct(v: &mut Vec<f64>, p: f64) -> f64 {
    if v.is_empty() { return f64::NAN; }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p) as usize]
}

pub fn run_sink(name: &str, rx: Receiver<Chunk>, shared: Arc<Shared>, wav_path: &str, csv_path: &str) -> Result<()> {
    let spec = hound::WavSpec { channels: 1, sample_rate: OUT_RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut wav = hound::WavWriter::create(wav_path, spec)?;
    let mut csv = std::io::BufWriter::new(std::fs::File::create(csv_path)?);
    writeln!(csv, "stream,t_s,out_frames,cb_latency_ms,newest_sample_age_ms,rms_dbfs,peak")?;

    let t_start = Instant::now();
    let mut rs: Option<(FftFixedIn<f32>, usize)> = None;
    let mut inbuf: Vec<f32> = Vec::new();
    let mut inbuf_t0: u64 = 0; // capture ns of inbuf[0]
    let mut cur_sr = 0f64;
    let mut cb_lat: Vec<f64> = vec![];
    let mut e2e_lat: Vec<f64> = vec![];
    let mut sec_max_db = -120f32;
    let mut sec_cb: Vec<f64> = vec![];
    let mut last_print = Instant::now();
    let mut out_total = 0usize;
    let mut chunks_in = 0usize;
    let mut pending_ts: VecDeque<()> = VecDeque::new();
    let _ = &mut pending_ts;

    loop {
        let chunk = match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(c) => c,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        chunks_in += 1;
        let n = chunk.samples.len();
        shared.total_in_frames.fetch_add(n as u64, Ordering::Relaxed);
        let ms = n as f64 / chunk.sr * 1000.0;
        if chunk.samples.iter().all(|v| *v == 0.0) {
            shared.zero_ms.fetch_add(ms as u64, Ordering::Relaxed);
        } else {
            shared.zero_ms.store(0, Ordering::Relaxed);
        }
        // latency: callback time minus capture time of the LAST sample in the buffer
        let end_cap = chunk.cap_ns as f64 + n as f64 / chunk.sr * 1e9;
        let cb_ms = (chunk.deliver_ns as f64 - end_cap) / 1e6;
        cb_lat.push(cb_ms);
        sec_cb.push(cb_ms);

        if (chunk.sr - cur_sr).abs() > 1.0 || rs.is_none() {
            eprintln!("[{name}] sample rate -> {} Hz (resampler rebuilt)", chunk.sr);
            cur_sr = chunk.sr;
            rs = Some((FftFixedIn::<f32>::new(chunk.sr as usize, OUT_RATE as usize, CHUNK_IN, 2, 1)?, chunk.sr as usize));
            inbuf.clear();
        }
        if inbuf.is_empty() { inbuf_t0 = chunk.cap_ns; }
        inbuf.extend_from_slice(&chunk.samples);

        let (r, sr_in) = rs.as_mut().unwrap();
        while inbuf.len() >= r.input_frames_next() {
            let need = r.input_frames_next();
            let block: Vec<f32> = inbuf.drain(..need).collect();
            let out = r.process(&[block], None)?;
            let o = &out[0];
            // now: when the resampled block is available to us
            let now_ns = crate::clock::now_ns();
            let newest_cap = inbuf_t0 as f64 + (need as f64 - 1.0) / *sr_in as f64 * 1e9;
            let e2e_ms = (now_ns as f64 - newest_cap) / 1e6;
            inbuf_t0 += (need as f64 / *sr_in as f64 * 1e9) as u64;
            e2e_lat.push(e2e_ms);
            let db = rms_dbfs(o);
            let peak = o.iter().fold(0f32, |a, b| a.max(b.abs()));
            sec_max_db = sec_max_db.max(db);
            for s in o {
                wav.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)?;
            }
            out_total += o.len();
            writeln!(csv, "{name},{:.3},{},{:.2},{:.2},{:.1},{:.4}", t_start.elapsed().as_secs_f64(), o.len(), cb_ms, e2e_ms, db, peak)?;
        }
        if last_print.elapsed() >= Duration::from_secs(1) {
            let m = sec_cb.iter().sum::<f64>() / sec_cb.len().max(1) as f64;
            eprintln!("[{name}] t={:5.1}s rms_max={:6.1} dBFS cb_lat_mean={:5.2} ms zero_run={} ms rate={} Hz",
                t_start.elapsed().as_secs_f64(), sec_max_db, m, shared.zero_ms.load(Ordering::Relaxed), cur_sr);
            sec_max_db = -120.0; sec_cb.clear(); last_print = Instant::now();
        }
    }
    wav.finalize()?;
    csv.flush()?;
    eprintln!("[{name}] DONE: {chunks_in} callbacks, {:.2} s of 16 kHz audio -> {wav_path}", out_total as f64 / OUT_RATE as f64);
    eprintln!("[{name}] callback latency ms: p50={:.2} p95={:.2} max={:.2}", pct(&mut cb_lat, 0.5), pct(&mut cb_lat, 0.95), pct(&mut cb_lat, 1.0));
    eprintln!("[{name}] newest-sample age at 16k-output ms (includes {CHUNK_IN}-frame resampler accumulation): p50={:.2} p95={:.2} max={:.2}", pct(&mut e2e_lat, 0.5), pct(&mut e2e_lat, 0.95), pct(&mut e2e_lat, 1.0));
    Ok(())
}

/// Goertzel magnitude (RMS amplitude estimate) at `freq` for a 16 kHz signal.
pub fn goertzel(x: &[f32], freq: f64, sr: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * freq / sr;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0f64, 0f64);
    for v in x { let s = *v as f64 + c * s1 - s2; s2 = s1; s1 = s; }
    let p = s1 * s1 + s2 * s2 - c * s1 * s2;
    (p.max(0.0)).sqrt() * 2.0 / x.len() as f64
}
