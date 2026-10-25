//! On-demand model downloads into app data: resumable, SHA-256 pinned, with progress.
//! Nothing is bundled with the installer.
//!
//! How a file is fetched (each rule exists because of a failure seen in the field):
//! - the file is requested in bounded `Range` slices, so every request has a hard time limit and a
//!   dead connection costs one slice, not the whole file;
//! - a stream that delivers nothing for `stall` is abandoned and resumed from the last byte on disk
//!   (the old code had no body timeout at all: a stalled connection sat at 0 MB/s);
//! - a `206` is accepted only when its `Content-Range` starts exactly at the bytes already on disk,
//!   a `200` (the server ignores `Range`) restarts the file from byte 0;
//! - failures back off (1 s, 2 s, 4 s, ... capped) and only count while no new byte arrives;
//! - a checksum mismatch drops the file and downloads it again (bounded), reported as `Repairing`;
//! - one destination is downloaded by one installer at a time: two concurrent installs of the same
//!   `.part` file corrupt it (two writers, and one deletes the file under the other).
use lowk_types::{AppError, ErrorCode, InstallPhase, Result};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct DownloadSpec {
    /// File name inside the engine's model directory.
    pub file: String,
    pub url: String,
    /// Lower-case hex SHA-256 pinned in the app.
    pub sha256: String,
    pub size_bytes: u64,
    pub license: String,
    pub license_url: String,
}

impl DownloadSpec {
    pub fn size_mb(&self) -> u32 {
        (self.size_bytes / (1024 * 1024)) as u32
    }
}

/// Progress callback: (file, phase, done_bytes_of_file, size_of_file).
pub type ProgressFn<'a> = &'a mut dyn FnMut(&str, InstallPhase, u64, u64);

/// Network behaviour of a download (tests shrink the times).
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    /// No byte for this long = the connection is dead: reconnect and resume.
    pub stall: Duration,
    /// Size of one `Range` slice.
    pub slice: u64,
    /// First retry delay; doubles up to `backoff_max`.
    pub backoff: Duration,
    pub backoff_max: Duration,
    /// Failures in a row without a single new byte before giving up on the file.
    pub max_failures: u32,
    /// How many times a file that fails its checksum is downloaded again.
    pub max_repairs: u32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            stall: Duration::from_secs(20),
            slice: 32 * 1024 * 1024,
            backoff: Duration::from_secs(1),
            backoff_max: Duration::from_secs(20),
            max_failures: 8,
            max_repairs: 2,
        }
    }
}

/// Overall progress of one install that downloads several files (the live model, the VAD model and,
/// with background correction, Whisper). The total is the sum of every file, known up front; the
/// reported figure never goes down, also when a file is downloaded again after a failed checksum.
#[derive(Debug)]
pub struct InstallTotals {
    total: u64,
    finished: HashMap<String, u64>,
    high: u64,
}

impl InstallTotals {
    pub fn new(total: u64) -> Self {
        Self {
            total,
            finished: HashMap::new(),
            high: 0,
        }
    }
    pub fn of_specs(specs: &[DownloadSpec]) -> Self {
        Self::new(specs.iter().map(|s| s.size_bytes).sum())
    }
    pub fn total(&self) -> u64 {
        self.total
    }
    /// Feeds one per-file event; returns `(done, total)` of the whole install.
    pub fn update(&mut self, file: &str, phase: InstallPhase, done: u64, size: u64) -> (u64, u64) {
        if phase == InstallPhase::Ready {
            self.finished.insert(file.to_owned(), size);
        }
        let inflight = if self.finished.contains_key(file) {
            0
        } else {
            done.min(size)
        };
        let now = (self.finished.values().sum::<u64>() + inflight).min(self.total);
        self.high = self.high.max(now);
        (self.high, self.total)
    }
}

/// Destinations being downloaded right now by this process.
fn inflight() -> &'static Mutex<HashSet<PathBuf>> {
    static S: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

struct Claim(PathBuf);

impl Claim {
    fn take(p: &Path) -> Option<Claim> {
        inflight()
            .lock()
            .ok()?
            .insert(p.to_path_buf())
            .then(|| Claim(p.to_path_buf()))
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if let Ok(mut g) = inflight().lock() {
            g.remove(&self.0);
        }
    }
}

/// `<data root>/models/<engine id>/`.
#[derive(Clone, Debug)]
pub struct ModelStore {
    /// `None` = follow the app's data root (it can be moved at runtime), `Some` = a fixed folder.
    root: Option<PathBuf>,
    tuning: Tuning,
}

impl ModelStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: Some(root),
            tuning: Tuning::default(),
        }
    }
    pub fn with_tuning(mut self, t: Tuning) -> Self {
        self.tuning = t;
        self
    }
    /// Follows the data root (`<data root>/models`), also after the user moves it.
    pub fn default_location() -> Option<Self> {
        lowk_platform::dirs::models().map(|_| Self {
            root: None,
            tuning: Tuning::default(),
        })
    }
    pub fn root(&self) -> PathBuf {
        self.root
            .clone()
            .or_else(lowk_platform::dirs::models)
            .unwrap_or_default()
    }
    pub fn dir(&self, engine: &str) -> PathBuf {
        self.root().join(engine)
    }
    pub fn path(&self, engine: &str, file: &str) -> PathBuf {
        self.dir(engine).join(file)
    }

    /// Present with the pinned size. (The hash was verified when the file was installed;
    /// re-hashing 750 MB on every start would cost seconds.)
    pub fn installed(&self, engine: &str, specs: &[DownloadSpec]) -> bool {
        specs.iter().all(|s| {
            fs::metadata(self.path(engine, &s.file))
                .map(|m| m.len() == s.size_bytes)
                .unwrap_or(false)
        })
    }

    /// Full integrity check (hashes every file).
    pub fn verify(&self, engine: &str, specs: &[DownloadSpec]) -> bool {
        specs
            .iter()
            .all(|s| sha256_file(&self.path(engine, &s.file)).is_ok_and(|h| h == s.sha256))
    }

    pub fn remove(&self, engine: &str) -> Result<()> {
        let d = self.dir(engine);
        if d.exists() {
            fs::remove_dir_all(&d).map_err(io_err)?;
        }
        Ok(())
    }

    /// Downloads whatever is missing, resuming `.part` files, then verifies each file. A file that
    /// fails its checksum is downloaded again (`max_repairs` times) before the install fails.
    pub fn install(
        &self,
        engine: &str,
        specs: &[DownloadSpec],
        cancel: &AtomicBool,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        fs::create_dir_all(self.dir(engine)).map_err(io_err)?;
        for s in specs {
            let dst = self.path(engine, &s.file);
            if fs::metadata(&dst)
                .map(|m| m.len() == s.size_bytes)
                .unwrap_or(false)
            {
                progress(&s.file, InstallPhase::Ready, s.size_bytes, s.size_bytes);
                continue;
            }
            let Some(_claim) = Claim::take(&dst) else {
                return Err(AppError::new(ErrorCode::DownloadFailed)
                    .with_param("reason", "busy")
                    .with_detail(format!("{} is already being downloaded", s.file)));
            };
            let part = dst.with_extension("part");
            self.fetch_verified(s, &part, cancel, progress)?;
            fs::rename(&part, &dst).map_err(io_err)?;
            progress(&s.file, InstallPhase::Ready, s.size_bytes, s.size_bytes);
        }
        Ok(())
    }

    fn fetch_verified(
        &self,
        s: &DownloadSpec,
        part: &Path,
        cancel: &AtomicBool,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let mut repairs = 0;
        loop {
            self.fetch_bytes(s, part, cancel, progress)?;
            progress(&s.file, InstallPhase::Verifying, s.size_bytes, s.size_bytes);
            if sha256_file(part).map_err(io_err)? == s.sha256 {
                return Ok(());
            }
            let _ = fs::remove_file(part);
            repairs += 1;
            tracing::warn!(file = %s.file, repairs, "checksum mismatch; downloading the file again");
            if repairs > self.tuning.max_repairs {
                return Err(AppError::new(ErrorCode::DownloadFailed)
                    .with_param("reason", "checksum")
                    .with_detail(s.file.clone()));
            }
            progress(&s.file, InstallPhase::Repairing, 0, s.size_bytes);
        }
    }

    /// Makes `part` exactly `size_bytes` long, resuming whatever it already holds.
    fn fetch_bytes(
        &self,
        s: &DownloadSpec,
        part: &Path,
        cancel: &AtomicBool,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let t = self.tuning;
        let agent = agent_for(s.url.as_str());
        let mut fails = 0u32;
        // The server answered a `Range` request with the whole file once: stop asking for slices.
        let mut ranges_ignored = false;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(cancelled());
            }
            let mut have = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
            if have > s.size_bytes {
                let _ = fs::remove_file(part);
                have = 0;
            }
            if have == s.size_bytes {
                // Complete (a previous run died while verifying): a Range past the end would be
                // answered with 416. Go straight to the checksum.
                progress(&s.file, InstallPhase::Downloading, have, s.size_bytes);
                return Ok(());
            }
            let mut moved = 0u64;
            let r = fetch_slice(
                &agent,
                s,
                part,
                have,
                &t,
                &mut ranges_ignored,
                cancel,
                &mut moved,
                &mut |done| progress(&s.file, InstallPhase::Downloading, done, s.size_bytes),
            );
            let mut failed = false;
            match r {
                Ok(()) => fails = 0,
                Err(Fail::Fatal(e)) => return Err(e),
                Err(Fail::Restart(why)) => {
                    tracing::warn!(file = %s.file, why, "download restarts from the beginning");
                    let _ = fs::remove_file(part);
                    failed = true;
                    fails = if moved > 0 { 1 } else { fails + 1 };
                    if fails > t.max_failures {
                        return Err(AppError::new(ErrorCode::Network).with_detail(why));
                    }
                }
                Err(Fail::Transient(why)) => {
                    failed = true;
                    // a request that moved bytes before it broke is progress, not a failure streak
                    fails = if moved > 0 { 1 } else { fails + 1 };
                    if fails > t.max_failures {
                        return Err(AppError::new(ErrorCode::Network).with_detail(why));
                    }
                    tracing::warn!(file = %s.file, fails, why, "download interrupted; resuming");
                }
            }
            if failed {
                let have = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
                progress(&s.file, InstallPhase::Retrying, have, s.size_bytes);
                let wait = (t.backoff * 2u32.saturating_pow(fails - 1)).min(t.backoff_max);
                sleep_unless_cancelled(wait, cancel)?;
            }
        }
    }
}

fn cancelled() -> AppError {
    AppError::new(ErrorCode::DownloadFailed).with_param("reason", "cancelled")
}

fn sleep_unless_cancelled(d: Duration, cancel: &AtomicBool) -> Result<()> {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        std::thread::sleep(Duration::from_millis(50).min(end - Instant::now()));
    }
    Ok(())
}

fn io_err(e: std::io::Error) -> AppError {
    AppError::new(ErrorCode::Io).with_detail(e.to_string())
}

pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Why one request ended early.
enum Fail {
    /// Network trouble: resume from the bytes on disk after a pause.
    Transient(String),
    /// The bytes on disk cannot be trusted (server answered from another offset): start over.
    Restart(String),
    /// Retrying will not help (404, the file changed on the server, cancelled).
    Fatal(AppError),
}

fn agent_for(url: &str) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .unversioned_rustls_crypto_provider(super::tls_provider())
        .root_certs(ureq::tls::RootCerts::WebPki)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(lowk_platform::proxy::for_url(url).and_then(|p| ureq::Proxy::new(&p).ok()))
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .build()
        .into()
}

enum Msg {
    Data(Vec<u8>),
    Eof,
    Err(String),
}

/// `Content-Range: bytes a-b/total` -> (a, b, total).
fn parse_content_range(v: &str) -> Option<(u64, u64, Option<u64>)> {
    let v = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = v.split_once('/')?;
    let (a, b) = range.trim().split_once('-')?;
    Some((a.parse().ok()?, b.parse().ok()?, total.trim().parse().ok()))
}

/// One request: the slice starting at `have` (the whole file when the server ignores `Range`).
/// Writes into `part`; `moved` counts the bytes that arrived, also when the request fails.
#[allow(clippy::too_many_arguments)]
fn fetch_slice(
    agent: &ureq::Agent,
    s: &DownloadSpec,
    part: &Path,
    have: u64,
    t: &Tuning,
    ranges_ignored: &mut bool,
    cancel: &AtomicBool,
    moved: &mut u64,
    on_progress: &mut dyn FnMut(u64),
) -> std::result::Result<(), Fail> {
    let expect = s.size_bytes;
    let slice_end = (have + t.slice).min(expect); // exclusive
                                                  // The body budget bounds a connection nobody notices is dead: a slice at >= 64 KiB/s, or the
                                                  // whole file at >= 32 KiB/s once the server is known to ignore `Range`.
    let budget = if *ranges_ignored {
        Duration::from_secs((expect / (32 * 1024)).max(120))
    } else {
        Duration::from_secs(((slice_end - have) / (64 * 1024)).max(30))
    };
    let mut req = agent.get(&s.url).config().timeout_recv_body(Some(budget)).build();
    if !*ranges_ignored {
        req = req.header("Range", format!("bytes={have}-{}", slice_end - 1));
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(416)) => return Err(Fail::Restart("416 range not satisfiable".into())),
        Err(ureq::Error::StatusCode(c)) if (400..500).contains(&c) && c != 408 && c != 429 => {
            return Err(Fail::Fatal(
                AppError::new(ErrorCode::DownloadFailed)
                    .with_param("reason", "http")
                    .with_detail(format!("HTTP {c} for {}", s.file)),
            ))
        }
        Err(e) => return Err(Fail::Transient(e.to_string())),
    };
    let status = resp.status().as_u16();
    let (start, wanted) = match status {
        206 => {
            let cr = resp
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range);
            let Some((a, b, total)) = cr else {
                return Err(Fail::Restart("206 without a usable Content-Range".into()));
            };
            if a != have || b < a || b >= expect {
                // Appending this would corrupt the file: the bytes are not the ones we asked for.
                return Err(Fail::Restart(format!("206 for bytes {a}-{b}, asked for {have}-")));
            }
            if total.is_some_and(|n| n != expect) {
                return Err(size_changed(s, total.unwrap_or(0)));
            }
            (have, (b - a + 1).min(slice_end - have))
        }
        200 => {
            // `Range` ignored: the body is the whole file.
            *ranges_ignored = true;
            let cl = resp
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            if cl.is_some_and(|n| n != expect) {
                return Err(size_changed(s, cl.unwrap_or(0)));
            }
            (0, expect)
        }
        other => return Err(Fail::Transient(format!("unexpected HTTP {other}"))),
    };
    let mut out = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(part)
        .map_err(|e| Fail::Fatal(io_err(e)))?;
    out.set_len(start).map_err(|e| Fail::Fatal(io_err(e)))?;
    out.seek(SeekFrom::Start(start))
        .map_err(|e| Fail::Fatal(io_err(e)))?;

    // The body is read on its own thread so that a silent connection can be abandoned: the thread
    // ends by itself when its read fails (body budget) or it finds nobody listening.
    let (tx, rx) = sync_channel::<Msg>(8);
    let mut reader = resp.into_body().into_reader();
    let spawned = std::thread::Builder::new()
        .name("lowk-dl-read".into())
        .spawn(move || loop {
            let mut b = vec![0u8; 128 * 1024];
            match reader.read(&mut b) {
                Ok(0) => {
                    let _ = tx.send(Msg::Eof);
                    break;
                }
                Ok(n) => {
                    b.truncate(n);
                    if tx.send(Msg::Data(b)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Msg::Err(e.to_string()));
                    break;
                }
            }
        });
    if let Err(e) = spawned {
        return Err(Fail::Transient(format!("cannot start the reader: {e}")));
    }
    let mut got = 0u64;
    let mut last_data = Instant::now();
    let mut last_report = Instant::now();
    let res = loop {
        if cancel.load(Ordering::Relaxed) {
            break Err(Fail::Fatal(cancelled()));
        }
        match rx.recv_timeout(Duration::from_millis(100).min(t.stall)) {
            Ok(Msg::Data(b)) => {
                last_data = Instant::now();
                let take = (b.len() as u64).min(wanted - got) as usize;
                if let Err(e) = out.write_all(&b[..take]) {
                    break Err(Fail::Fatal(io_err(e)));
                }
                got += take as u64;
                *moved += take as u64;
                if last_report.elapsed() >= Duration::from_millis(200) {
                    last_report = Instant::now();
                    on_progress(start + got);
                }
                if got >= wanted {
                    break Ok(());
                }
            }
            Ok(Msg::Eof) => break Ok(()),
            Ok(Msg::Err(e)) => break Err(Fail::Transient(e)),
            Err(RecvTimeoutError::Timeout) => {
                if last_data.elapsed() >= t.stall {
                    break Err(Fail::Transient(format!(
                        "no data for {:.1} s; reconnecting",
                        t.stall.as_secs_f32()
                    )));
                }
            }
            Err(RecvTimeoutError::Disconnected) => break Err(Fail::Transient("reader ended".into())),
        }
    };
    drop(rx);
    let flushed = out.flush();
    on_progress(start + got);
    res?;
    flushed.map_err(|e| Fail::Fatal(io_err(e)))?;
    if got < wanted {
        return Err(Fail::Transient(format!("short body {got}/{wanted}")));
    }
    Ok(())
}

fn size_changed(s: &DownloadSpec, n: u64) -> Fail {
    Fail::Fatal(
        AppError::new(ErrorCode::DownloadFailed)
            .with_param("reason", "size")
            .with_detail(format!(
                "{} is {n} bytes on the server, expected {}",
                s.file, s.size_bytes
            )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    /// What the mock server does with one request.
    #[derive(Clone, Default)]
    struct Plan {
        /// Answer `200` with the whole body even though a range was asked for.
        ignore_range: bool,
        /// Flip every served byte (a corrupt transfer).
        corrupt: bool,
        /// Close the connection after this many body bytes (declared length stays the full one).
        cut_after: Option<usize>,
        /// Send this many body bytes, then go silent for a few seconds.
        stall_after: Option<usize>,
        /// Answer a range request with a 206 that starts at this offset instead.
        wrong_offset: Option<usize>,
    }

    struct Mock {
        url: String,
        requests: Arc<AtomicUsize>,
        ranges: Arc<Mutex<Vec<String>>>,
    }

    /// HTTP server with `Range` support; `plan(n)` decides how request number `n` (from 0) is served.
    fn serve(body: Vec<u8>, plan: impl Fn(usize) -> Plan + Send + Sync + 'static) -> Mock {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let requests = Arc::new(AtomicUsize::new(0));
        let ranges = Arc::new(Mutex::new(vec![]));
        let (rq, rg) = (requests.clone(), ranges.clone());
        let plan = Arc::new(plan);
        std::thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(mut s) = conn else { break };
                let n = rq.fetch_add(1, Ordering::SeqCst);
                let (body, plan, rg) = (body.clone(), plan.clone(), rg.clone());
                std::thread::spawn(move || {
                    let mut r = BufReader::new(s.try_clone().unwrap());
                    let mut range: Option<(usize, Option<usize>)> = None;
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            rg.lock().unwrap().push(v.trim().to_string());
                            let (a, b) = v.trim().split_once('-').unwrap();
                            range = Some((a.parse().unwrap(), b.parse().ok()));
                        }
                    }
                    let p = plan(n);
                    let total = body.len();
                    let (head, data): (String, Vec<u8>) = match range {
                        Some((from, to)) if !p.ignore_range => {
                            let to = to.map_or(total - 1, |t| t.min(total - 1));
                            let served = p.wrong_offset.unwrap_or(from);
                            let slice = &body[served..=(served + (to - from)).min(total - 1)];
                            (
                                format!(
                                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                                    slice.len(),
                                    served,
                                    served + slice.len() - 1,
                                    total
                                ),
                                slice.to_vec(),
                            )
                        }
                        _ => (
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
                            ),
                            body.clone(),
                        ),
                    };
                    let mut data = data;
                    if p.corrupt {
                        for b in &mut data {
                            *b ^= 0xFF;
                        }
                    }
                    let _ = s.write_all(head.as_bytes());
                    let send = p
                        .cut_after
                        .or(p.stall_after)
                        .map_or(data.len(), |k| k.min(data.len()));
                    let _ = s.write_all(&data[..send]);
                    let _ = s.flush();
                    if p.stall_after.is_some() {
                        std::thread::sleep(Duration::from_secs(4));
                    }
                });
            }
        });
        Mock {
            url: format!("http://127.0.0.1:{port}/m.bin"),
            requests,
            ranges,
        }
    }

    fn fast() -> Tuning {
        Tuning {
            stall: Duration::from_millis(400),
            slice: 100_000,
            backoff: Duration::from_millis(10),
            backoff_max: Duration::from_millis(40),
            max_failures: 4,
            max_repairs: 2,
        }
    }

    fn spec(url: String, body: &[u8], sha: Option<String>) -> DownloadSpec {
        DownloadSpec {
            file: "m.bin".into(),
            url,
            sha256: sha.unwrap_or_else(|| hex(&Sha256::digest(body))),
            size_bytes: body.len() as u64,
            license: "MIT".into(),
            license_url: String::new(),
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lowk-models-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn data(n: u32, m: u32) -> Vec<u8> {
        (0..n).map(|i| (i % m) as u8).collect()
    }

    type Events = Vec<(InstallPhase, u64)>;

    fn run(st: &ModelStore, specs: &[DownloadSpec]) -> (Result<()>, Events) {
        let mut ev: Events = vec![];
        let r = st.install("e", specs, &AtomicBool::new(false), &mut |_, p, d, _| {
            ev.push((p, d))
        });
        (r, ev)
    }

    #[test]
    fn installs_verifies_and_reports_progress_in_slices() {
        let body = data(450_000, 251);
        let m = serve(body.clone(), |_| Plan::default());
        let st = ModelStore::new(tmp("ok")).with_tuning(fast());
        let specs = vec![spec(m.url.clone(), &body, None)];
        assert!(!st.installed("e", &specs));
        let (r, ev) = run(&st, &specs);
        r.unwrap();
        assert!(st.installed("e", &specs) && st.verify("e", &specs));
        assert_eq!(ev.last().map(|e| e.0), Some(InstallPhase::Ready));
        assert!(ev.iter().any(|e| e.0 == InstallPhase::Verifying));
        // 450 000 bytes in slices of 100 000 = 5 requests, each asking for its own range
        assert_eq!(m.requests.load(Ordering::SeqCst), 5);
        assert_eq!(m.ranges.lock().unwrap()[1], "100000-199999");
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn resumes_a_partial_download() {
        let body = data(400_000, 253);
        let m = serve(body.clone(), |_| Plan::default());
        let st = ModelStore::new(tmp("resume")).with_tuning(fast());
        let specs = vec![spec(m.url.clone(), &body, None)];
        fs::create_dir_all(st.dir("e")).unwrap();
        fs::write(st.path("e", "m.part"), &body[..150_000]).unwrap();
        run(&st, &specs).0.unwrap();
        assert!(st.verify("e", &specs));
        assert_eq!(m.ranges.lock().unwrap()[0], "150000-249999");
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn complete_part_file_is_verified_not_requested_again() {
        let body = data(50_000, 249);
        let st = ModelStore::new(tmp("fullpart")).with_tuning(Tuning {
            max_repairs: 0,
            max_failures: 0,
            ..fast()
        });
        // the server is never contacted: a dead URL proves it
        let specs = vec![spec("http://127.0.0.1:1/none".into(), &body, None)];
        fs::create_dir_all(st.dir("e")).unwrap();
        fs::write(st.path("e", "m.part"), &body).unwrap();
        run(&st, &specs).0.unwrap();
        assert!(st.verify("e", &specs));
        // a corrupt complete part is rejected and removed
        let _ = fs::remove_file(st.path("e", "m.bin"));
        fs::write(st.path("e", "m.part"), vec![0u8; body.len()]).unwrap();
        let e = run(&st, &specs).0.unwrap_err();
        assert_eq!(e.params.get("reason").map(String::as_str), Some("checksum"));
        assert!(!st.path("e", "m.part").exists());
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn server_that_ignores_range_restarts_the_file_instead_of_appending() {
        // The corruption the old code was exposed to: a 200 answer to a resume request appended
        // after the bytes already on disk gives a wrong file.
        let body = data(300_000, 247);
        let m = serve(body.clone(), |_| Plan {
            ignore_range: true,
            ..Plan::default()
        });
        let st = ModelStore::new(tmp("noRange")).with_tuning(fast());
        let specs = vec![spec(m.url.clone(), &body, None)];
        fs::create_dir_all(st.dir("e")).unwrap();
        fs::write(st.path("e", "m.part"), &body[..120_000]).unwrap();
        let (r, ev) = run(&st, &specs);
        r.unwrap();
        assert!(st.verify("e", &specs));
        assert!(!ev.iter().any(|e| e.0 == InstallPhase::Repairing));
        assert_eq!(fs::metadata(st.path("e", "m.bin")).unwrap().len(), 300_000);
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn a_206_from_the_wrong_offset_is_never_appended() {
        let body = data(300_000, 239);
        // the second request is answered from byte 0 although a later range was asked for
        let m = serve(body.clone(), |n| Plan {
            wrong_offset: (n == 1).then_some(0),
            ..Plan::default()
        });
        let st = ModelStore::new(tmp("wrongOffset")).with_tuning(fast());
        let specs = vec![spec(m.url.clone(), &body, None)];
        let (r, ev) = run(&st, &specs);
        r.unwrap();
        assert!(st.verify("e", &specs));
        // no checksum round happened: the bad slice was rejected before it touched the file
        assert!(!ev.iter().any(|e| e.0 == InstallPhase::Repairing));
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn a_dropped_connection_resumes_from_the_last_byte() {
        let body = data(350_000, 241);
        let m = serve(body.clone(), |n| Plan {
            cut_after: (n == 1).then_some(40_000),
            ..Plan::default()
        });
        let st = ModelStore::new(tmp("drop")).with_tuning(fast());
        let specs = vec![spec(m.url.clone(), &body, None)];
        let (r, ev) = run(&st, &specs);
        r.unwrap();
        assert!(st.verify("e", &specs));
        assert!(ev.iter().any(|e| e.0 == InstallPhase::Retrying));
        // the retry asked for the bytes after the 40 000 that had arrived: 100 000 + 40 000
        assert!(m.ranges.lock().unwrap().iter().any(|r| r.starts_with("140000-")));
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn a_stalled_connection_is_abandoned_and_resumed() {
        let body = data(350_000, 233);
        let m = serve(body.clone(), |n| Plan {
            stall_after: (n == 1).then_some(30_000),
            ..Plan::default()
        });
        let st = ModelStore::new(tmp("stall")).with_tuning(fast());
        let specs = vec![spec(m.url.clone(), &body, None)];
        let t0 = Instant::now();
        let (r, ev) = run(&st, &specs);
        r.unwrap();
        // far less than the 4 s the silent server would have held the connection
        assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
        assert!(st.verify("e", &specs));
        assert!(ev.iter().any(|e| e.0 == InstallPhase::Retrying));
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn checksum_mismatch_downloads_the_file_again_and_succeeds() {
        let body = data(200_000, 229);
        // the first complete pass (2 slices) is corrupt, the second one is clean
        let m = serve(body.clone(), |n| Plan {
            corrupt: n < 2,
            ..Plan::default()
        });
        let st = ModelStore::new(tmp("repair")).with_tuning(fast());
        let specs = vec![spec(m.url.clone(), &body, None)];
        let (r, ev) = run(&st, &specs);
        r.unwrap();
        assert!(st.verify("e", &specs));
        assert!(ev.iter().any(|e| e.0 == InstallPhase::Repairing));
        assert_eq!(ev.last().map(|e| e.0), Some(InstallPhase::Ready));
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn checksum_mismatch_that_never_heals_fails_cleaned_up() {
        let body = vec![7u8; 10_000];
        let m = serve(body.clone(), |_| Plan::default());
        let st = ModelStore::new(tmp("bad")).with_tuning(fast());
        let specs = vec![spec(m.url.clone(), &body, Some("00".repeat(32)))];
        let e = run(&st, &specs).0.unwrap_err();
        assert_eq!(e.code, ErrorCode::DownloadFailed);
        assert_eq!(e.params.get("reason").map(String::as_str), Some("checksum"));
        assert!(!st.path("e", "m.bin").exists() && !st.path("e", "m.part").exists());
        // first pass + max_repairs more
        assert_eq!(m.requests.load(Ordering::SeqCst), 3);
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn a_missing_file_fails_at_once_without_retries() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            for c in l.incoming() {
                let Ok(mut s) = c else { break };
                h.fetch_add(1, Ordering::SeqCst);
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                while r.read_line(&mut line).unwrap_or(0) > 0 && !line.trim().is_empty() {
                    line.clear();
                }
                let _ =
                    s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        let body = vec![1u8; 1000];
        let st = ModelStore::new(tmp("404")).with_tuning(fast());
        let specs = vec![spec(format!("http://127.0.0.1:{port}/x"), &body, None)];
        let e = run(&st, &specs).0.unwrap_err();
        assert_eq!(e.params.get("reason").map(String::as_str), Some("http"));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn a_dead_server_gives_up_after_bounded_retries() {
        let body = vec![1u8; 1000];
        let st = ModelStore::new(tmp("dead")).with_tuning(fast());
        let specs = vec![spec("http://127.0.0.1:1/none".into(), &body, None)];
        let t0 = Instant::now();
        let e = run(&st, &specs).0.unwrap_err();
        assert_eq!(e.code, ErrorCode::Network);
        assert!(t0.elapsed() < Duration::from_secs(5));
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn two_installs_of_the_same_file_do_not_run_together() {
        // The field failure: the same model started twice wrote one `.part` from two threads.
        let body = data(300_000, 227);
        let m = serve(body.clone(), |_| Plan {
            stall_after: Some(10_000),
            ..Plan::default()
        });
        let st = Arc::new(ModelStore::new(tmp("twice")).with_tuning(Tuning {
            stall: Duration::from_millis(1500),
            ..fast()
        }));
        let specs = Arc::new(vec![spec(m.url.clone(), &body, None)]);
        let (a_st, a_specs) = (st.clone(), specs.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let a_stop = stop.clone();
        let first = std::thread::spawn(move || a_st.install("e", &a_specs, &a_stop, &mut |_, _, _, _| {}));
        std::thread::sleep(Duration::from_millis(300));
        let second = run(&st, &specs).0.unwrap_err();
        assert_eq!(second.params.get("reason").map(String::as_str), Some("busy"));
        stop.store(true, Ordering::SeqCst);
        let _ = first.join();
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn cancel_stops_a_download() {
        let body = data(300_000, 223);
        let m = serve(body.clone(), |_| Plan::default());
        let st = ModelStore::new(tmp("cancel")).with_tuning(fast());
        let specs = vec![spec(m.url.clone(), &body, None)];
        let cancel = AtomicBool::new(true);
        let e = st
            .install("e", &specs, &cancel, &mut |_, _, _, _| {})
            .unwrap_err();
        assert_eq!(e.params.get("reason").map(String::as_str), Some("cancelled"));
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn install_totals_are_known_up_front_and_never_go_down() {
        let mut t = InstallTotals::new(1000);
        assert_eq!(t.update("a", InstallPhase::Downloading, 100, 600), (100, 1000));
        assert_eq!(t.update("a", InstallPhase::Downloading, 600, 600), (600, 1000));
        assert_eq!(t.update("a", InstallPhase::Verifying, 600, 600), (600, 1000));
        // the checksum failed: the file starts again from 0, the figure holds
        assert_eq!(t.update("a", InstallPhase::Repairing, 0, 600), (600, 1000));
        assert_eq!(t.update("a", InstallPhase::Downloading, 300, 600), (600, 1000));
        assert_eq!(t.update("a", InstallPhase::Downloading, 600, 600), (600, 1000));
        assert_eq!(t.update("a", InstallPhase::Ready, 600, 600), (600, 1000));
        // a repeated Ready does not count twice
        assert_eq!(t.update("a", InstallPhase::Ready, 600, 600), (600, 1000));
        assert_eq!(t.update("b", InstallPhase::Downloading, 100, 400), (700, 1000));
        assert_eq!(t.update("b", InstallPhase::Ready, 400, 400), (1000, 1000));
    }

    /// Real HTTPS download of the 310 MB model (TLS provider, redirects, SHA pin). `--ignored`.
    #[test]
    #[ignore = "needs network"]
    fn downloads_embeddinggemma_over_https() {
        let st = ModelStore::new(tmp("net"));
        let specs: Vec<_> = crate::embedder::local::gemma_assets();
        st.install("e", &specs, &AtomicBool::new(false), &mut |_, _, _, _| {})
            .unwrap();
        assert!(st.verify("e", &specs));
        let _ = fs::remove_dir_all(st.root());
    }
}
