//! On-demand model downloads into app data: resumable, SHA-256 pinned, with progress.
//! Nothing is bundled with the installer.
use lowk_types::{AppError, ErrorCode, InstallPhase, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Progress callback: (file, phase, done_bytes, total_bytes).
pub type ProgressFn<'a> = &'a mut dyn FnMut(&str, InstallPhase, u64, u64);

/// `<app data>/models/<engine id>/`.
#[derive(Clone, Debug)]
pub struct ModelStore {
    root: PathBuf,
}

impl ModelStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    /// Default location (`<app data>/models`).
    pub fn default_location() -> Option<Self> {
        lowk_platform::dirs::models().map(Self::new)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn dir(&self, engine: &str) -> PathBuf {
        self.root.join(engine)
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

    /// Downloads whatever is missing, resuming `.part` files, then verifies each file.
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
            let part = dst.with_extension("part");
            // Network hiccups resume from the `.part` file instead of starting over.
            let mut attempt = 0;
            loop {
                attempt += 1;
                match download(&s.url, &part, s.size_bytes, cancel, &mut |done| {
                    progress(&s.file, InstallPhase::Downloading, done, s.size_bytes)
                }) {
                    Ok(()) => break,
                    Err(e) if e.code == ErrorCode::Network && attempt < 4 => {
                        tracing::warn!(file = %s.file, attempt, "download interrupted; resuming");
                        std::thread::sleep(Duration::from_secs(2));
                    }
                    Err(e) => return Err(e),
                }
            }
            progress(&s.file, InstallPhase::Verifying, s.size_bytes, s.size_bytes);
            let got = sha256_file(&part).map_err(io_err)?;
            if got != s.sha256 {
                let _ = fs::remove_file(&part);
                tracing::warn!(file = %s.file, "checksum mismatch");
                return Err(AppError::new(ErrorCode::DownloadFailed)
                    .with_param("reason", "checksum")
                    .with_detail(s.file.clone()));
            }
            fs::rename(&part, &dst).map_err(io_err)?;
            progress(&s.file, InstallPhase::Ready, s.size_bytes, s.size_bytes);
        }
        Ok(())
    }
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

/// GET `url` into `part`, resuming when the file already has bytes.
fn download(
    url: &str,
    part: &Path,
    expect: u64,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64),
) -> Result<()> {
    let have = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    let have = if have > expect { 0 } else { have };
    if have == expect && expect > 0 {
        // A complete `.part` (the app died while verifying): a Range request past the end is
        // answered with 416, which would loop forever. Go straight to the checksum.
        on_progress(have);
        return Ok(());
    }
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .unversioned_rustls_crypto_provider(crate::cloud::tls_provider())
        .root_certs(ureq::tls::RootCerts::WebPki)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(lowk_platform::proxy::for_url(url).and_then(|p| ureq::Proxy::new(&p).ok()))
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .build()
        .into();
    let mut req = agent.get(url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| {
        tracing::warn!(error = %e, "model download request failed");
        AppError::new(ErrorCode::Network).with_detail(e.to_string())
    })?;
    let resumed = resp.status().as_u16() == 206;
    let mut out = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(!resumed)
        .open(part)
        .map_err(io_err)?;
    let mut done = if resumed {
        out.seek(SeekFrom::End(0)).map_err(io_err)?
    } else {
        0
    };
    let mut body = resp.into_body();
    let mut r = body.as_reader();
    let mut buf = vec![0u8; 256 * 1024];
    let mut last = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::new(ErrorCode::DownloadFailed).with_param("reason", "cancelled"));
        }
        let n = r
            .read(&mut buf)
            .map_err(|e| AppError::new(ErrorCode::Network).with_detail(e.to_string()))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(io_err)?;
        done += n as u64;
        if last.elapsed() >= Duration::from_millis(200) {
            last = Instant::now();
            on_progress(done);
        }
    }
    out.flush().map_err(io_err)?;
    on_progress(done);
    if done != expect {
        return Err(AppError::new(ErrorCode::Network).with_detail(format!("short download {done}/{expect}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    /// One-file HTTP server with `Range` support; serves `n` requests.
    fn serve(body: Vec<u8>, n: usize) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for _ in 0..n {
                let (mut s, _) = l.accept().unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut from = 0usize;
                loop {
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = v.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                let part = &body[from..];
                let head = if from > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        part.len(),
                        from,
                        body.len() - 1,
                        body.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        part.len()
                    )
                };
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(part);
            }
        });
        format!("http://127.0.0.1:{port}/m.bin")
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

    #[test]
    fn installs_verifies_and_reports_progress() {
        let body: Vec<u8> = (0..600_000u32).map(|i| (i % 251) as u8).collect();
        let st = ModelStore::new(tmp("ok"));
        let specs = vec![spec(serve(body.clone(), 1), &body, None)];
        assert!(!st.installed("e", &specs));
        let mut phases = vec![];
        st.install("e", &specs, &AtomicBool::new(false), &mut |_, p, _, _| {
            phases.push(p)
        })
        .unwrap();
        assert!(st.installed("e", &specs) && st.verify("e", &specs));
        assert_eq!(phases.last(), Some(&InstallPhase::Ready));
        assert!(phases.contains(&InstallPhase::Verifying));
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn resumes_a_partial_download() {
        let body: Vec<u8> = (0..400_000u32).map(|i| (i % 253) as u8).collect();
        let st = ModelStore::new(tmp("resume"));
        let specs = vec![spec(serve(body.clone(), 1), &body, None)];
        fs::create_dir_all(st.dir("e")).unwrap();
        fs::write(st.path("e", "m.part"), &body[..150_000]).unwrap();
        st.install("e", &specs, &AtomicBool::new(false), &mut |_, _, _, _| {})
            .unwrap();
        assert!(st.verify("e", &specs));
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn complete_part_file_is_verified_not_requested_again() {
        let body: Vec<u8> = (0..50_000u32).map(|i| (i % 249) as u8).collect();
        let st = ModelStore::new(tmp("fullpart"));
        // the server is never contacted: a dead URL proves it
        let specs = vec![spec("http://127.0.0.1:1/none".into(), &body, None)];
        fs::create_dir_all(st.dir("e")).unwrap();
        fs::write(st.path("e", "m.part"), &body).unwrap();
        st.install("e", &specs, &AtomicBool::new(false), &mut |_, _, _, _| {})
            .unwrap();
        assert!(st.verify("e", &specs));
        // a corrupt complete part is rejected and removed
        let _ = fs::remove_file(st.path("e", "m.bin"));
        fs::write(st.path("e", "m.part"), vec![0u8; body.len()]).unwrap();
        let e = st
            .install("e", &specs, &AtomicBool::new(false), &mut |_, _, _, _| {})
            .unwrap_err();
        assert_eq!(e.params.get("reason").map(String::as_str), Some("checksum"));
        assert!(!st.path("e", "m.part").exists());
        let _ = fs::remove_dir_all(st.root());
    }

    #[test]
    fn checksum_mismatch_is_rejected_and_cleaned() {
        let body = vec![7u8; 10_000];
        let st = ModelStore::new(tmp("bad"));
        let specs = vec![spec(serve(body.clone(), 1), &body, Some("00".repeat(32)))];
        let e = st
            .install("e", &specs, &AtomicBool::new(false), &mut |_, _, _, _| {})
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::DownloadFailed);
        assert_eq!(e.params.get("reason").map(String::as_str), Some("checksum"));
        assert!(!st.path("e", "m.bin").exists() && !st.path("e", "m.part").exists());
        let _ = fs::remove_dir_all(st.root());
    }

    /// Real HTTPS download (TLS provider, redirects, SHA pin). Run with `--ignored`.
    #[test]
    #[ignore = "needs network"]
    fn downloads_the_vad_model_over_https() {
        let st = ModelStore::new(tmp("net"));
        let specs: Vec<_> = crate::local::nemotron_assets()
            .into_iter()
            .filter(|a| a.file.contains("silero"))
            .collect();
        st.install("e", &specs, &AtomicBool::new(false), &mut |_, _, _, _| {})
            .unwrap();
        assert!(st.verify("e", &specs));
        let _ = fs::remove_dir_all(st.root());
    }
}
