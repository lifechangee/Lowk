//! Host side of the sidecar protocol: a worker process that
//! owns the model; PCM goes in as length-prefixed binary frames, events come back as JSON
//! lines. A crashed or hung worker is restarted without touching the session.
use crate::diag::StreamStats;
use crate::{SttConfig, SttEngine};
use lowk_types::*;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const KIND_AUDIO: u8 = 1;
pub const KIND_CONTROL: u8 = 2;
pub const KIND_UTTERANCE: u8 = 3;

/// Worker restarts before the engine gives up (and reports a non-recoverable error).
const MAX_RESTARTS: u32 = 3;
/// 20 s of 20 ms frames.
const QUEUE_FRAMES: usize = 1000;

#[derive(Clone, Debug)]
pub struct SidecarSpec {
    pub exe: PathBuf,
    pub args: Vec<String>,
    /// `nice` increment on unix (background correction runs at low priority).
    pub nice: i32,
}

pub fn frame_bytes(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(5 + payload.len());
    v.push(kind);
    v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    v.extend_from_slice(payload);
    v
}

pub fn pcm_i16(pcm: &[f32], out: &mut Vec<u8>) {
    for s in pcm {
        let q = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&q.to_le_bytes());
    }
}

pub fn spawn_worker(spec: &SidecarSpec) -> std::io::Result<Child> {
    let mut cmd = if spec.nice > 0 {
        lowk_platform::process::background_command(&spec.exe)
    } else {
        lowk_platform::process::command(&spec.exe)
    };
    cmd.args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    if spec.nice != 0 {
        use std::os::unix::process::CommandExt;
        let n = spec.nice;
        // SAFETY: nice(2) / setpriority(2) are async-signal-safe; they run between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                extern "C" {
                    fn nice(inc: i32) -> i32;
                    fn setpriority(which: i32, who: u32, prio: i32) -> i32;
                }
                nice(n);
                #[cfg(target_os = "macos")]
                {
                    // PRIO_DARWIN_PROCESS / PRIO_DARWIN_BG: background QoS (efficiency cores,
                    // throttled I/O) so the correction worker never competes with live streams.
                    setpriority(4, 0, 0x1000);
                }
                #[cfg(not(target_os = "macos"))]
                let _ = setpriority;
                Ok(())
            });
        }
    }
    cmd.spawn()
}

/// One line per worker start: which compute backend the model landed on. A CPU run on a machine that
/// should have a GPU (Windows builds ship Vulkan) is called out explicitly: it is the first thing to check
/// when the live text lags.
pub fn backend_note(backend: &str) -> (bool, String) {
    if backend_is_cpu(backend) {
        (
            true,
            format!(
                "STT worker runs on the CPU (backend '{backend}'): no GPU backend available (no Vulkan/CUDA driver, \
                 or a CPU-only worker build). Live text may lag; the Deepgram cloud engine is the fallback."
            ),
        )
    } else {
        (false, format!("STT worker runs on the GPU backend '{backend}'"))
    }
}

fn log_backend(stream: Stream, backend: &str) {
    let (cpu, msg) = backend_note(backend);
    if cpu && cfg!(windows) {
        tracing::warn!(?stream, "{msg}");
    } else {
        tracing::info!(?stream, "{msg}");
    }
}

const STDERR_TAIL_LINES: usize = 20;
pub use lowk_platform::process::{tail_text, StderrTail};

/// Drains a child's stderr into the debug log (never into user-visible errors) and returns its tail.
pub fn drain_stderr(child: &mut Child, tag: &'static str) -> StderrTail {
    match child.stderr.take() {
        Some(e) => lowk_platform::process::tail_stderr(
            LogLines {
                inner: BufReader::new(e),
                tag,
            },
            STDERR_TAIL_LINES,
        ),
        None => Default::default(),
    }
}

/// Pass-through reader that also writes each stderr line to the debug log.
struct LogLines<R: BufRead> {
    inner: R,
    tag: &'static str,
}
impl<R: BufRead> std::io::Read for LogLines<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            for l in String::from_utf8_lossy(&buf[..n]).lines() {
                tracing::debug!(target: "lowk_stt::worker", "{}: {l}", self.tag);
            }
        }
        Ok(n)
    }
}

enum Out {
    Audio(MonoNs, Vec<f32>),
    Flush,
    Stop,
}

struct Shared {
    stats: Arc<StreamStats>,
    utt_base: AtomicU64,
    ready: AtomicBool,
    alive: AtomicBool,
    /// Compute backend the worker reported in its `ready` event (empty until then).
    backend: Mutex<Option<String>>,
}

pub struct SidecarEngine {
    desc: EngineDescriptor,
    stream: Stream,
    spec: SidecarSpec,
    lookahead_ms: u32,
    shared: Arc<Shared>,
    tx: Option<SyncSender<Out>>,
    sup: Option<JoinHandle<()>>,
    sink: Option<Arc<dyn SttSink>>,
    overflow_reported: bool,
    ready_timeout: Duration,
    /// Extra args appended after the spec args (language etc. from `SttConfig`).
    lang_arg: Mutex<Option<String>>,
}

impl SidecarEngine {
    pub fn new(desc: EngineDescriptor, stream: Stream, spec: SidecarSpec, lookahead_ms: u32) -> Self {
        Self {
            desc,
            stream,
            spec,
            lookahead_ms,
            shared: Arc::new(Shared {
                stats: Arc::new(StreamStats::default()),
                utt_base: AtomicU64::new(0),
                ready: AtomicBool::new(false),
                alive: AtomicBool::new(false),
                backend: Mutex::new(None),
            }),
            tx: None,
            sup: None,
            sink: None,
            overflow_reported: false,
            ready_timeout: READY_TIMEOUT,
            lang_arg: Mutex::new(None),
        }
    }
}

/// How long a worker may take to report `ready` before the start is declared failed.
pub const READY_TIMEOUT: Duration = Duration::from_secs(60);

impl SidecarEngine {
    /// Overrides the readiness deadline ([`READY_TIMEOUT`]); tests use a short one.
    pub fn with_ready_timeout(mut self, d: Duration) -> Self {
        self.ready_timeout = d;
        self
    }
}

/// `["ru","en"]` -> `auto`; `["ru"]` -> `ru-RU`; `["pl"]` -> `pl-PL`.
pub fn worker_lang(langs: &[LangTag]) -> String {
    lowk_types::langpolicy::decoder_lang(langs)
}

impl SttEngine for SidecarEngine {
    fn descriptor(&self) -> &EngineDescriptor {
        &self.desc
    }

    fn start(&mut self, cfg: &SttConfig, sink: Arc<dyn SttSink>) -> Result<()> {
        if self.tx.is_some() {
            return Err(AppError::new(ErrorCode::SessionBusy));
        }
        *self.lang_arg.lock().unwrap() = Some(worker_lang(&cfg.langs));
        let mut spec = self.spec.clone();
        if !spec.args.iter().any(|a| a == "--lang") {
            spec.args.push("--lang".into());
            spec.args.push(worker_lang(&cfg.langs));
        }
        if !spec.args.iter().any(|a| a == "--allowed") {
            spec.args.push("--allowed".into());
            spec.args.push(cfg.langs.join(","));
        }
        if !spec.exe.exists() {
            return Err(
                AppError::new(ErrorCode::NotFound).with_detail(format!("worker {}", spec.exe.display()))
            );
        }
        let (tx, rx) = sync_channel(QUEUE_FRAMES);
        self.tx = Some(tx);
        self.sink = Some(sink.clone());
        let shared = self.shared.clone();
        let stream = self.stream;
        let ready_timeout = self.ready_timeout;
        self.sup = Some(
            std::thread::Builder::new()
                .name(format!("lowk-stt-{stream:?}"))
                .spawn(move || supervise(spec, stream, sink, rx, shared, ready_timeout))
                .map_err(|e| AppError::new(ErrorCode::SttEngine).with_detail(e.to_string()))?,
        );
        Ok(())
    }

    fn push(&mut self, frame: &Frame16k) -> Result<()> {
        let Some(tx) = &self.tx else { return Ok(()) };
        let end = frame.t_mono_ns + (frame.pcm.len() as u64) * 1_000_000_000 / 16_000;
        self.shared.stats.note_pushed(end);
        match tx.try_send(Out::Audio(frame.t_mono_ns, frame.pcm.clone())) {
            Ok(()) => Ok(()),
            Err(_) => {
                self.shared.stats.dropped_frames.fetch_add(1, Ordering::Relaxed);
                if !self.overflow_reported {
                    self.overflow_reported = true;
                    if let Some(s) = &self.sink {
                        s.error(SttError {
                            stream: self.stream,
                            message: "stt backlog: dropping audio".into(),
                            recoverable: true,
                            code: None,
                        });
                    }
                }
                Err(AppError::new(ErrorCode::SttBacklog))
            }
        }
    }

    fn flush(&mut self) -> Result<()> {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Out::Flush);
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Out::Stop);
        }
        if let Some(h) = self.sup.take() {
            let _ = h.join();
        }
        self.sink = None;
        Ok(())
    }

    fn backlog_ms(&self) -> u32 {
        self.shared.stats.backlog_ms()
    }

    fn lookahead_ms(&self) -> u32 {
        self.lookahead_ms
    }

    fn stats(&self) -> Option<StreamDiag> {
        Some(self.shared.stats.diag(self.stream))
    }

    fn is_ready(&self) -> bool {
        self.shared.ready.load(Ordering::Relaxed)
    }

    fn backend(&self) -> Option<String> {
        self.shared.backend.lock().unwrap().clone()
    }
}

impl Drop for SidecarEngine {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn parse_event(v: &serde_json::Value, stream: Stream, base: u64) -> Option<SttEvent> {
    if v["ev"] != "stt" {
        return None;
    }
    // S1: a token boundary inside a multi-byte letter must never reach the transcript as «�».
    let text = lowk_types::text::clean_speech(v["text"].as_str()?);
    if text.is_empty() {
        return None;
    }
    Some(SttEvent {
        stream,
        utt: base + v["utt"].as_u64()?,
        rev: v["rev"].as_u64()? as u32,
        kind: if v["kind"] == "final" {
            SttKind::Final
        } else {
            SttKind::Partial
        },
        text,
        lang: v["lang"].as_str().map(str::to_owned),
        t_start_ns: v["t_start_ns"].as_u64()?,
        t_audio_end_ns: v["t_audio_end_ns"].as_u64()?,
        t_emit_ns: now_mono_ns(),
    })
}

fn supervise(
    spec: SidecarSpec,
    stream: Stream,
    sink: Arc<dyn SttSink>,
    rx: Receiver<Out>,
    sh: Arc<Shared>,
    ready_timeout: Duration,
) {
    let mut restarts = 0u32;
    loop {
        let mut child = match spawn_worker(&spec) {
            Ok(c) => c,
            Err(e) => {
                sink.error(SttError {
                    stream,
                    message: format!("cannot start stt worker: {e}"),
                    recoverable: false,
                    code: None,
                });
                return;
            }
        };
        sh.alive.store(true, Ordering::SeqCst);
        sh.ready.store(false, Ordering::SeqCst);
        let tail = drain_stderr(&mut child, "stt");
        let started = Instant::now();
        let mut timed_out = false;
        let mut stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let reader = {
            let sh = sh.clone();
            let sink = sink.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(std::result::Result::ok) {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                        continue;
                    };
                    match v["ev"].as_str() {
                        Some("ready") => {
                            let backend = v["backend"].as_str().unwrap_or("");
                            log_backend(stream, backend);
                            *sh.backend.lock().unwrap() = Some(backend.to_owned());
                            sh.ready.store(true, Ordering::SeqCst)
                        }
                        Some("ack") => {
                            if let Some(t) = v["t_ns"].as_u64() {
                                sh.stats.note_acked(t);
                            }
                        }
                        Some("stt") => {
                            if let Some(ev) = parse_event(&v, stream, sh.utt_base.load(Ordering::Relaxed)) {
                                sh.stats.note_event(&ev);
                                sink.emit(ev);
                            }
                        }
                        Some("error") => sink.error(SttError {
                            stream,
                            message: v["message"].as_str().unwrap_or("worker error").to_owned(),
                            recoverable: v["fatal"] != true,
                            code: None,
                        }),
                        _ => {}
                    }
                }
                sh.alive.store(false, Ordering::SeqCst);
            })
        };

        let mut stopping = false;
        let mut buf = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Out::Audio(t, pcm)) => {
                    buf.clear();
                    buf.extend_from_slice(&t.to_le_bytes());
                    pcm_i16(&pcm, &mut buf);
                    if stdin.write_all(&frame_bytes(KIND_AUDIO, &buf)).is_err() {
                        break;
                    }
                }
                Ok(Out::Flush) => {
                    if stdin
                        .write_all(&frame_bytes(KIND_CONTROL, br#"{"op":"flush"}"#))
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Out::Stop) | Err(RecvTimeoutError::Disconnected) => {
                    let _ = stdin.write_all(&frame_bytes(KIND_CONTROL, br#"{"op":"stop"}"#));
                    stopping = true;
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            if !sh.alive.load(Ordering::SeqCst) {
                break;
            }
            if !sh.ready.load(Ordering::SeqCst) && started.elapsed() > ready_timeout {
                timed_out = true;
                break;
            }
        }
        drop(stdin);
        if stopping {
            // let the worker finalize and print the last events
            let t0 = Instant::now();
            while sh.alive.load(Ordering::SeqCst) && t0.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return;
        }
        // crashed or hung up
        let exit = if timed_out {
            let _ = child.kill();
            child.wait().ok()
        } else {
            // already gone or hung up on us; do not wait forever for a worker that still runs
            let st = child.try_wait().ok().flatten();
            let _ = child.kill();
            st.or_else(|| child.wait().ok())
        };
        let _ = reader.join();
        restarts += 1;
        sh.stats.restarts.store(restarts, Ordering::Relaxed);
        let cause = if timed_out {
            Some(ErrorCode::EngineLoadTimeout)
        } else {
            exit.and_then(|st| lowk_platform::process::exit_cause(&st))
        };
        // A missing instruction set or a model that never loads does not heal by restarting.
        let recoverable = cause.is_none() && restarts <= MAX_RESTARTS;
        tracing::warn!(
            ?stream,
            ?cause,
            exit = ?exit.map(|s| s.to_string()),
            stderr_tail = %tail_text(&tail),
            "stt worker failed"
        );
        sink.error(SttError {
            stream,
            message: match cause {
                Some(ErrorCode::EngineLoadTimeout) => {
                    format!("stt worker not ready after {} s", ready_timeout.as_secs())
                }
                Some(_) => "stt worker crashed: unsupported processor (illegal instruction)".into(),
                None => "stt worker exited unexpectedly".into(),
            },
            recoverable,
            code: cause,
        });
        if !recoverable {
            // keep draining so push() never blocks, until stop
            while let Ok(m) = rx.recv() {
                if matches!(m, Out::Stop) {
                    break;
                }
            }
            return;
        }
        sh.utt_base.fetch_add(1_000_000, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(500));
        // audio queued while the worker was down is stale: drop it
        let mut stop = false;
        while let Ok(m) = rx.try_recv() {
            if matches!(m, Out::Stop) {
                stop = true;
            }
        }
        if stop {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use lowk_types::fakes::arc_sink;

    #[test]
    fn backend_note_flags_cpu() {
        assert!(backend_note("cpu").0);
        assert!(backend_note("").0);
        assert!(backend_note("CPU").1.contains("CPU"));
        assert!(!backend_note("vulkan").0);
        assert!(!backend_note("metal").0);
    }

    #[test]
    fn framing_roundtrip() {
        let f = frame_bytes(KIND_CONTROL, b"hi");
        assert_eq!(f, vec![2, 2, 0, 0, 0, b'h', b'i']);
        let mut v = vec![];
        pcm_i16(&[0.0, 1.0, -1.0, 2.0], &mut v);
        assert_eq!(v, vec![0, 0, 0xff, 0x7f, 0x01, 0x80, 0xff, 0x7f]);
    }

    #[test]
    fn lang_mapping() {
        assert_eq!(worker_lang(&["ru".into()]), "ru-RU");
        assert_eq!(worker_lang(&["ru".into(), "en".into()]), "auto");
        assert_eq!(worker_lang(&[]), "auto");
    }

    #[cfg(unix)]
    #[test]
    fn fake_worker_events_and_restart() {
        // A shell "worker": prints ready, one partial and one final per run, then exits
        // when stdin closes. The first run exits at once (crash) so the host must restart.
        let dir = std::env::temp_dir().join(format!("lowk-sidecar-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("ran");
        let script = dir.join("w.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n\
                 echo '{{\"ev\":\"ready\"}}'\n\
                 if [ ! -f {m} ]; then touch {m}; exit 3; fi\n\
                 echo '{{\"ev\":\"stt\",\"kind\":\"partial\",\"utt\":1,\"rev\":0,\"text\":\"при\",\"t_start_ns\":1,\"t_audio_end_ns\":2}}'\n\
                 echo '{{\"ev\":\"stt\",\"kind\":\"final\",\"utt\":1,\"rev\":1,\"text\":\"привет\",\"t_start_ns\":1,\"t_audio_end_ns\":3}}'\n\
                 cat >/dev/null\n",
                m = marker.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let desc = EngineDescriptor {
            id: "t".into(),
            name: "t".into(),
            kind: EngineKind::Local,
            languages: vec![],
            streaming: StreamingMode::Native,
            size_mb: 0,
            license: "".into(),
        };
        let mut e = SidecarEngine::new(
            desc,
            Stream::Me,
            SidecarSpec {
                exe: script,
                args: vec![],
                nice: 0,
            },
            0,
        );
        let sink = arc_sink();
        e.start(
            &SttConfig {
                stream: Stream::Me,
                langs: vec![],
                vocab_hints: vec![],
            },
            sink.clone(),
        )
        .unwrap();
        let t0 = Instant::now();
        while sink.events.lock().unwrap().len() < 2 && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        e.stop().unwrap();
        let evs = sink.events.lock().unwrap().clone();
        assert_eq!(evs.len(), 2, "events: {evs:?}");
        assert_eq!(evs[1].kind, SttKind::Final);
        // utterance ids after a restart live in a new range
        assert!(evs[0].utt >= 1_000_000);
        assert_eq!(sink.errors.lock().unwrap().len(), 1);
        assert!(sink.errors.lock().unwrap()[0].recoverable);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn not_ready_while_the_worker_loads_and_audio_is_queued() {
        // A shell "worker" that needs 1.2 s to load: not ready until then; the audio pushed
        // meanwhile is still delivered afterwards (it waits in the pipe, nothing is dropped).
        let dir = std::env::temp_dir().join(format!("lowk-sidecar-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("w.sh");
        let got = dir.join("got");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 1.2\necho '{{\"ev\":\"ready\"}}'\ncat > {}\n",
                got.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let desc = EngineDescriptor {
            id: "t".into(),
            name: "t".into(),
            kind: EngineKind::Local,
            languages: vec![],
            streaming: StreamingMode::Native,
            size_mb: 0,
            license: "".into(),
        };
        let mut e = SidecarEngine::new(
            desc,
            Stream::Me,
            SidecarSpec {
                exe: script,
                args: vec![],
                nice: 0,
            },
            0,
        );
        let cfg = SttConfig {
            stream: Stream::Me,
            langs: vec![],
            vocab_hints: vec![],
        };
        e.start(&cfg, arc_sink()).unwrap();
        assert!(!e.is_ready());
        e.push(&Frame16k {
            stream: Stream::Me,
            t_mono_ns: 1,
            pcm: vec![0.1; 320],
        })
        .unwrap();
        let t0 = Instant::now();
        while !e.is_ready() && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(e.is_ready());
        assert!(t0.elapsed() > Duration::from_millis(500), "was ready too early");
        e.stop().unwrap();
        let n = std::fs::metadata(&got).map(|m| m.len()).unwrap_or(0);
        assert!(n > 320 * 2, "queued audio reached the worker: {n} bytes");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn worker_that_never_reports_ready_times_out_with_a_code() {
        let dir = std::env::temp_dir().join(format!("lowk-sidecar-hang-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("w.sh");
        std::fs::write(&script, "#!/bin/sh\necho loading model >&2\nexec sleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let desc = EngineDescriptor {
            id: "t".into(),
            name: "t".into(),
            kind: EngineKind::Local,
            languages: vec![],
            streaming: StreamingMode::Native,
            size_mb: 0,
            license: "".into(),
        };
        let mut e = SidecarEngine::new(
            desc,
            Stream::Me,
            SidecarSpec {
                exe: script,
                args: vec![],
                nice: 0,
            },
            0,
        )
        .with_ready_timeout(Duration::from_millis(600));
        let sink = arc_sink();
        e.start(
            &SttConfig {
                stream: Stream::Me,
                langs: vec![],
                vocab_hints: vec![],
            },
            sink.clone(),
        )
        .unwrap();
        let t0 = Instant::now();
        while sink.errors.lock().unwrap().is_empty() && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        let errs = sink.errors.lock().unwrap().clone();
        assert_eq!(
            errs.first().and_then(|e| e.code),
            Some(ErrorCode::EngineLoadTimeout),
            "{errs:?}"
        );
        assert!(!errs[0].recoverable);
        e.stop().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn worker_killed_by_sigill_reports_cpu_unsupported() {
        let dir = std::env::temp_dir().join(format!("lowk-sidecar-ill-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("w.sh");
        std::fs::write(&script, "#!/bin/sh\nkill -ILL $$\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let desc = EngineDescriptor {
            id: "t".into(),
            name: "t".into(),
            kind: EngineKind::Local,
            languages: vec![],
            streaming: StreamingMode::Native,
            size_mb: 0,
            license: "".into(),
        };
        let mut e = SidecarEngine::new(
            desc,
            Stream::Me,
            SidecarSpec {
                exe: script,
                args: vec![],
                nice: 0,
            },
            0,
        );
        let sink = arc_sink();
        e.start(
            &SttConfig {
                stream: Stream::Me,
                langs: vec![],
                vocab_hints: vec![],
            },
            sink.clone(),
        )
        .unwrap();
        let t0 = Instant::now();
        while sink.errors.lock().unwrap().is_empty() && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        let errs = sink.errors.lock().unwrap().clone();
        assert_eq!(
            errs.first().and_then(|e| e.code),
            Some(ErrorCode::CpuUnsupported),
            "{errs:?}"
        );
        assert!(
            !errs[0].recoverable,
            "no restart loop on a missing instruction set"
        );
        e.stop().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }
}
