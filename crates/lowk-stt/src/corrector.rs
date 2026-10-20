//! Engine #2: background correction. Finalized utterances are re-transcribed by Whisper
//! large-v3-turbo at low priority; the better text replaces the stored one (as a newer
//! revision of the same utterance). It never blocks anything: jobs are skipped when the
//! queue is deep or the live engine is behind, and a stuck worker is simply restarted.
use crate::diag::Window;
use crate::sidecar::{
    drain_stderr, frame_bytes, pcm_i16, spawn_worker, SidecarSpec, KIND_CONTROL, KIND_UTTERANCE,
};
use lowk_types::*;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Jobs waiting beyond this are skipped (the call keeps its live text).
pub const MAX_PENDING: usize = 4;
/// Live-engine backlog above which corrections are skipped.
pub const BUSY_BACKLOG_MS: u32 = 1000;
const MIN_AUDIO_MS: usize = 700;
const MAX_AUDIO_MS: usize = 29_000;
const JOB_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct CorrectionJob {
    pub stream: Stream,
    pub utt: UttId,
    /// Revision of the final being corrected; the correction is `rev + 1`.
    pub rev: u32,
    pub t_start_ns: MonoNs,
    pub t_audio_end_ns: MonoNs,
    pub pcm: Vec<f32>,
    pub original: String,
    /// Single configured language (`ru`), `None` = let Whisper detect.
    pub lang: Option<String>,
}

pub trait CorrectionSink: Send + Sync {
    /// `Some(final with rev + 1)` when the text changed, `None` when kept / failed.
    fn done(&self, stream: Stream, utt: UttId, result: Option<SttEvent>);
}

pub trait Corrector: Send + Sync {
    /// Starts (loads) the worker in the background; cheap to call again.
    fn start(&self) -> Result<()>;
    /// `false` = skipped (nothing will be reported to the sink).
    fn submit(&self, job: CorrectionJob, sink: Arc<dyn CorrectionSink>, live_backlog_ms: u32) -> bool;
    /// Jobs accepted but not yet reported.
    fn pending(&self) -> usize;
    /// Waits up to `wait` for outstanding jobs, then shuts the worker down.
    fn stop(&self, wait: Duration);
    /// (corrected, skipped, latency of a correction from submit to result)
    fn stats(&self) -> (u32, u32, Percentiles);
}

/// Rejects hallucinated / truncated Whisper output.
pub fn plausible(original: &str, new: &str) -> bool {
    let (o, n) = (original.trim().chars().count(), new.trim().chars().count());
    if n == 0 {
        return false;
    }
    if o < 12 {
        return n < 200;
    }
    let r = n as f32 / o as f32;
    (0.45..=2.2).contains(&r)
}

#[derive(Default)]
struct Counters {
    corrected: AtomicU32,
    skipped: AtomicU32,
    pending: AtomicUsize,
    lag: Window,
}

struct Queued {
    job: CorrectionJob,
    sink: Arc<dyn CorrectionSink>,
    at: Instant,
}

struct Running {
    tx: SyncSender<Option<Queued>>,
    thread: std::thread::JoinHandle<()>,
}

pub struct WhisperCorrector {
    spec: SidecarSpec,
    c: Arc<Counters>,
    run: Mutex<Option<Running>>,
}

impl WhisperCorrector {
    pub fn new(mut spec: SidecarSpec) -> Self {
        spec.nice = spec.nice.max(15);
        Self {
            spec,
            c: Arc::new(Counters::default()),
            run: Mutex::new(None),
        }
    }
}

impl Corrector for WhisperCorrector {
    fn start(&self) -> Result<()> {
        let mut g = self.run.lock().unwrap();
        if g.is_some() {
            return Ok(());
        }
        if !self.spec.exe.exists() {
            return Err(AppError::new(ErrorCode::NotFound).with_detail("lowk-stt-worker"));
        }
        let (tx, rx) = sync_channel(MAX_PENDING + 2);
        let spec = self.spec.clone();
        let c = self.c.clone();
        let thread = std::thread::Builder::new()
            .name("lowk-stt-correct".into())
            .spawn(move || run_loop(spec, rx, c))
            .map_err(|e| AppError::new(ErrorCode::SttEngine).with_detail(e.to_string()))?;
        *g = Some(Running { tx, thread });
        Ok(())
    }

    fn submit(&self, job: CorrectionJob, sink: Arc<dyn CorrectionSink>, live_backlog_ms: u32) -> bool {
        let ms = job.pcm.len() / 16;
        let skip = live_backlog_ms > BUSY_BACKLOG_MS
            || self.c.pending.load(Ordering::SeqCst) >= MAX_PENDING
            || !(MIN_AUDIO_MS..=MAX_AUDIO_MS).contains(&ms);
        let g = self.run.lock().unwrap();
        let Some(r) = g.as_ref().filter(|_| !skip) else {
            self.c.skipped.fetch_add(1, Ordering::Relaxed);
            return false;
        };
        self.c.pending.fetch_add(1, Ordering::SeqCst);
        if r.tx
            .try_send(Some(Queued {
                job,
                sink,
                at: Instant::now(),
            }))
            .is_err()
        {
            self.c.pending.fetch_sub(1, Ordering::SeqCst);
            self.c.skipped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        true
    }

    fn pending(&self) -> usize {
        self.c.pending.load(Ordering::SeqCst)
    }

    fn stop(&self, wait: Duration) {
        let t0 = Instant::now();
        while self.pending() > 0 && t0.elapsed() < wait {
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Some(r) = self.run.lock().unwrap().take() {
            let _ = r.tx.try_send(None);
            drop(r.tx);
            let _ = r.thread.join();
        }
    }

    fn stats(&self) -> (u32, u32, Percentiles) {
        (
            self.c.corrected.load(Ordering::Relaxed),
            self.c.skipped.load(Ordering::Relaxed),
            self.c.lag.percentiles(),
        )
    }
}

struct Reply {
    id: u64,
    text: String,
    lang: Option<String>,
}

fn run_loop(spec: SidecarSpec, rx: Receiver<Option<Queued>>, c: Arc<Counters>) {
    let mut next_id = 0u64;
    let mut conn: Option<(std::process::Child, std::process::ChildStdin, Receiver<Reply>)> = None;
    loop {
        let q = match rx.recv() {
            Ok(Some(q)) => q,
            _ => break,
        };
        let finish = |q: &Queued, ev: Option<SttEvent>| {
            c.pending.fetch_sub(1, Ordering::SeqCst);
            q.sink.done(q.job.stream, q.job.utt, ev);
        };
        if conn.is_none() {
            match connect(&spec) {
                Some(x) => conn = Some(x),
                None => {
                    c.skipped.fetch_add(1, Ordering::Relaxed);
                    finish(&q, None);
                    continue;
                }
            }
        }
        let (child, stdin, replies) = conn.as_mut().unwrap();
        next_id += 1;
        let lang = q.job.lang.clone().unwrap_or_default();
        let mut p = Vec::with_capacity(9 + lang.len() + q.job.pcm.len() * 2);
        p.extend_from_slice(&next_id.to_le_bytes());
        p.push(lang.len().min(255) as u8);
        p.extend_from_slice(lang.as_bytes());
        pcm_i16(&q.job.pcm, &mut p);
        let mut reply = None;
        if stdin.write_all(&frame_bytes(KIND_UTTERANCE, &p)).is_ok() {
            let t0 = Instant::now();
            loop {
                match replies.recv_timeout(JOB_TIMEOUT.saturating_sub(t0.elapsed())) {
                    Ok(r) if r.id == next_id => {
                        reply = Some(r);
                        break;
                    }
                    Ok(_) => {}
                    Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
                }
            }
        }
        let Some(r) = reply else {
            tracing::warn!("correction worker failed or timed out; restarting it");
            let _ = child.kill();
            let _ = child.wait();
            conn = None;
            c.skipped.fetch_add(1, Ordering::Relaxed);
            finish(&q, None);
            continue;
        };
        c.lag
            .record(q.at.elapsed().as_millis().min(u32::MAX as u128) as u32);
        let text = r.text.trim().to_owned();
        let changed = text != q.job.original.trim() && plausible(&q.job.original, &text);
        let ev = changed.then(|| SttEvent {
            stream: q.job.stream,
            utt: q.job.utt,
            rev: q.job.rev + 1,
            kind: SttKind::Final,
            text,
            lang: r.lang,
            t_start_ns: q.job.t_start_ns,
            t_audio_end_ns: q.job.t_audio_end_ns,
            t_emit_ns: now_mono_ns(),
        });
        if changed {
            c.corrected.fetch_add(1, Ordering::Relaxed);
        }
        finish(&q, ev);
    }
    if let Some((mut child, mut stdin, _)) = conn {
        let _ = stdin.write_all(&frame_bytes(KIND_CONTROL, br#"{"op":"stop"}"#));
        drop(stdin);
        std::thread::sleep(Duration::from_millis(100));
        let _ = child.kill();
        let _ = child.wait();
    }
    // anything still queued after shutdown is dropped without a report
    while let Ok(Some(q)) = rx.try_recv() {
        c.pending.fetch_sub(1, Ordering::SeqCst);
        q.sink.done(q.job.stream, q.job.utt, None);
    }
}

fn connect(spec: &SidecarSpec) -> Option<(std::process::Child, std::process::ChildStdin, Receiver<Reply>)> {
    let mut child = spawn_worker(spec).ok()?;
    drain_stderr(&mut child, "correct");
    let stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = channel();
    let (ready_tx, ready_rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(std::result::Result::ok) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            match v["ev"].as_str() {
                Some("ready") => {
                    let _ = ready_tx.send(());
                }
                Some("corr") => {
                    let _ = tx.send(Reply {
                        id: v["id"].as_u64().unwrap_or(0),
                        text: v["text"].as_str().unwrap_or("").to_owned(),
                        lang: v["lang"].as_str().map(str::to_owned),
                    });
                }
                _ => {}
            }
        }
    });
    if ready_rx.recv_timeout(Duration::from_secs(90)).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    Some((child, stdin, rx))
}

type FakeFn = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Test double: answers every job with `f(original)` after `delay`.
pub struct FakeCorrector {
    pub f: FakeFn,
    pub delay: Duration,
    pending: Arc<AtomicUsize>,
    pub submitted: Arc<AtomicU32>,
}
impl FakeCorrector {
    pub fn new(delay: Duration, f: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Self {
        Self {
            f: Box::new(f),
            delay,
            pending: Arc::new(AtomicUsize::new(0)),
            submitted: Arc::new(AtomicU32::new(0)),
        }
    }
}
impl Corrector for FakeCorrector {
    fn start(&self) -> Result<()> {
        Ok(())
    }
    fn submit(&self, job: CorrectionJob, sink: Arc<dyn CorrectionSink>, live_backlog_ms: u32) -> bool {
        if live_backlog_ms > BUSY_BACKLOG_MS {
            return false;
        }
        self.submitted.fetch_add(1, Ordering::SeqCst);
        self.pending.fetch_add(1, Ordering::SeqCst);
        let out = (self.f)(&job.original);
        let (d, p) = (self.delay, self.pending.clone());
        std::thread::spawn(move || {
            std::thread::sleep(d);
            let ev = out.map(|text| SttEvent {
                stream: job.stream,
                utt: job.utt,
                rev: job.rev + 1,
                kind: SttKind::Final,
                text,
                lang: None,
                t_start_ns: job.t_start_ns,
                t_audio_end_ns: job.t_audio_end_ns,
                t_emit_ns: now_mono_ns(),
            });
            sink.done(job.stream, job.utt, ev);
            p.fetch_sub(1, Ordering::SeqCst);
        });
        true
    }
    fn pending(&self) -> usize {
        self.pending.load(Ordering::SeqCst)
    }
    fn stop(&self, wait: Duration) {
        let t0 = Instant::now();
        while self.pending() > 0 && t0.elapsed() < wait {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn stats(&self) -> (u32, u32, Percentiles) {
        (0, 0, Percentiles::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plausibility_guards_against_hallucinations() {
        assert!(plausible("привет как дела у вас", "Привет, как дела у вас?"));
        assert!(!plausible(
            "привет как дела у вас сегодня",
            "Спасибо за просмотр!!! Спасибо за просмотр!!! Спасибо за просмотр!!!"
        ));
        assert!(!plausible("привет как дела у вас сегодня", ""));
        assert!(!plausible("привет как дела у вас сегодня", "да"));
        assert!(plausible("да", "Да."));
    }

    #[test]
    fn submit_without_worker_is_skipped_not_blocking() {
        let c = WhisperCorrector::new(SidecarSpec {
            exe: "/nonexistent".into(),
            args: vec![],
            nice: 0,
        });
        assert!(c.start().is_err());
        struct S;
        impl CorrectionSink for S {
            fn done(&self, _: Stream, _: UttId, _: Option<SttEvent>) {}
        }
        let job = CorrectionJob {
            stream: Stream::Me,
            utt: 1,
            rev: 1,
            t_start_ns: 0,
            t_audio_end_ns: 1,
            pcm: vec![0.0; 32000],
            original: "x".into(),
            lang: None,
        };
        assert!(!c.submit(job, Arc::new(S), 0));
        assert_eq!(c.stats().1, 1);
    }
}
