//! Local EmbeddingGemma 2 (Q8_0 GGUF) through the `lowk-embed-worker` sidecar (llama.cpp).
//!
//! The sidecar is upstream llama.cpp `llama-server --embedding` (built by
//! `workers/lowk-embed-worker/build.sh`, shipped as `lowk-embed-worker`), reached over loopback
//! HTTP (`/health`, `/v1/embeddings`). A dead or hung worker is replaced once per call; a second
//! failure is `EmbedFailed`. The process is killed when the embedder is dropped.
use super::models::{DownloadSpec, ModelStore};
use super::Embedder;
use lowk_types::{
    AppError, EmbedderInfo, EngineProgress, ErrorCode, InstallPhase, Result, DEFAULT_EMBEDDER_ID,
};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// Directory name under `<app data>/models/` and the `engine` field of `engine://progress`.
pub const GEMMA_ENGINE_ID: &str = "embedder";
pub const GEMMA_FILE: &str = "embeddinggemma-2-Q8_0.gguf";
/// Pinned to a commit of the repository, not `main`.
const GEMMA_REV: &str = "58faf90940886df2d62be555c8765d00dac362c5";
pub const GEMMA_LICENSE_URL: &str = "https://ai.google.dev/gemma/apache_2";
/// Vector size stored in the index (MRL 768 -> 512; DECISIONS-M2).
pub const INDEX_DIM: usize = 512;

pub fn gemma_assets() -> Vec<DownloadSpec> {
    vec![DownloadSpec {
        file: GEMMA_FILE.into(),
        url: format!(
            "https://huggingface.co/ggml-org/embeddinggemma-2-GGUF/resolve/{GEMMA_REV}/{GEMMA_FILE}"
        ),
        sha256: "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135".into(),
        size_bytes: 309_855_456,
        license: "Apache-2.0".into(),
        license_url: GEMMA_LICENSE_URL.into(),
    }]
}

/// `embedder_info` payload.
pub fn gemma_info(installed: bool, installing: bool) -> EmbedderInfo {
    let a = &gemma_assets()[0];
    EmbedderInfo {
        id: DEFAULT_EMBEDDER_ID.into(),
        name: "EmbeddingGemma 2 (Q8_0)".into(),
        dim: INDEX_DIM as u32,
        size_mb: a.size_mb(),
        installed,
        installing,
        license: a.license.clone(),
        license_url: a.license_url.clone(),
    }
}

/// Path of the model file inside `store` (whether or not it is installed yet).
pub fn model_path(store: &ModelStore) -> PathBuf {
    store.path(GEMMA_ENGINE_ID, GEMMA_FILE)
}

pub fn is_installed(store: &ModelStore) -> bool {
    store.installed(GEMMA_ENGINE_ID, &gemma_assets())
}

/// Downloads the model (resumable, SHA-256 pinned) and reports `engine://progress` payloads
/// with `engine == "embedder"`. A failure is reported as `InstallPhase::Failed` and returned.
pub fn install(
    store: &ModelStore,
    cancel: &AtomicBool,
    on: &mut dyn FnMut(EngineProgress),
) -> Result<PathBuf> {
    let specs = gemma_assets();
    let r = store.install(
        GEMMA_ENGINE_ID,
        &specs,
        cancel,
        &mut |file, phase, done, total| {
            on(EngineProgress {
                engine: GEMMA_ENGINE_ID.into(),
                file: file.into(),
                phase,
                done_bytes: done,
                total_bytes: total,
                message: None,
            })
        },
    );
    if let Err(e) = &r {
        on(EngineProgress {
            engine: GEMMA_ENGINE_ID.into(),
            file: GEMMA_FILE.into(),
            phase: InstallPhase::Failed,
            done_bytes: 0,
            total_bytes: specs[0].size_bytes,
            message: Some(format!("{:?}", e.code)),
        });
    }
    r.map(|()| model_path(store))
}

/// Where the worker executable lives: `LOWK_EMBED_WORKER`, next to the app executable, in
/// `<app data>/engines`, and (debug builds) the in-repo build output.
pub fn find_worker() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "lowk-embed-worker.exe"
    } else {
        "lowk-embed-worker"
    };
    if let Ok(p) = std::env::var("LOWK_EMBED_WORKER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut c: Vec<PathBuf> = vec![];
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        c.push(d.join(name));
    }
    if let Some(d) = lowk_platform::dirs::engines() {
        c.push(d.join(name));
    }
    #[cfg(debug_assertions)]
    c.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../workers/lowk-embed-worker/target/release")
            .join(name),
    );
    c.into_iter().find(|p| p.is_file())
}

struct Proc {
    child: Child,
    port: u16,
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct LocalEmbedder {
    worker: PathBuf,
    model: PathBuf,
    dim: usize,
    id: String,
    proc: Mutex<Option<Proc>>,
    agent: ureq::Agent,
}

impl LocalEmbedder {
    pub fn new(worker: PathBuf, model: PathBuf, dim: usize) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(180)))
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            worker,
            model,
            dim,
            id: format!("{DEFAULT_EMBEDDER_ID}:{dim}"),
            proc: Mutex::new(None),
            agent,
        }
    }

    /// Starts `llama-server --embedding` on a free loopback port and waits until it serves.
    fn spawn(&self) -> Result<Proc> {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .map_err(|e| AppError::new(ErrorCode::EmbedFailed).with_detail(e.to_string()))?;
        let child = Command::new(&self.worker)
            .args(["-m"])
            .arg(&self.model)
            .args(["--embedding", "--host", "127.0.0.1", "--port"])
            .arg(port.to_string())
            // one slot; context and batch hold a whole chunk (chunks are <= ~700 real tokens)
            .args([
                "-c",
                "4096",
                "-b",
                "4096",
                "-ub",
                "4096",
                "-np",
                "1",
                "-ngl",
                "99",
                "--no-webui",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::new(ErrorCode::EmbedderMissing).with_detail(e.to_string()))?;
        let mut p = Proc { child, port };
        // loading 300 MB from a cold disk can take a while
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            if let Ok(Some(st)) = p.child.try_wait() {
                return Err(AppError::new(ErrorCode::EmbedFailed).with_detail(format!("worker exited: {st}")));
            }
            let ok = self
                .agent
                .get(&format!("http://127.0.0.1:{port}/health"))
                .call()
                .map(|r| r.status().as_u16() == 200)
                .unwrap_or(false);
            if ok {
                return Ok(p);
            }
            if Instant::now() > deadline {
                return Err(AppError::new(ErrorCode::EmbedFailed).with_detail("worker did not become ready"));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    fn call(&self, port: u16, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let body = serde_json::json!({ "input": texts, "encoding_format": "float" });
        let mut resp = self
            .agent
            .post(&format!("http://127.0.0.1:{port}/v1/embeddings"))
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|e| AppError::new(ErrorCode::EmbedFailed).with_detail(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| AppError::new(ErrorCode::EmbedFailed).with_detail(e.to_string()))?;
        if status != 200 {
            return Err(AppError::new(ErrorCode::EmbedFailed)
                .with_param("status", status)
                .with_detail(text.chars().take(200).collect::<String>()));
        }
        let v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AppError::new(ErrorCode::EmbedFailed).with_detail(e.to_string()))?;
        let mut rows: Vec<(usize, Vec<f32>)> = v["data"]
            .as_array()
            .ok_or_else(|| AppError::new(ErrorCode::EmbedFailed).with_detail("no data array"))?
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let idx = d["index"].as_u64().map(|x| x as usize).unwrap_or(i);
                let e = d["embedding"].as_array()?;
                Some((
                    idx,
                    e.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect(),
                ))
            })
            .collect();
        rows.sort_by_key(|r| r.0);
        // Matryoshka: the first `dim` values of a 768-d EmbeddingGemma vector are a valid
        // embedding of their own (the index re-normalises).
        if rows.len() != texts.len()
            || rows
                .iter()
                .any(|r| r.1.len() < self.dim || r.1.iter().any(|x| !x.is_finite()))
        {
            return Err(AppError::new(ErrorCode::EmbedFailed).with_detail("unexpected embeddings shape"));
        }
        Ok(rows.into_iter().map(|r| super::truncate(r.1, self.dim)).collect())
    }
}

impl Embedder for LocalEmbedder {
    fn id(&self) -> &str {
        &self.id
    }
    fn dim(&self) -> usize {
        self.dim
    }
    fn embed_docs(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let mut guard = self.proc.lock();
        for attempt in 0..2 {
            if guard.is_none() {
                *guard = Some(self.spawn()?);
            }
            let port = guard.as_ref().map(|p| p.port).unwrap_or(0);
            match self.call(port, texts) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    tracing::warn!(attempt, error = ?e.code, "embedder worker call failed");
                    *guard = None;
                    if attempt == 1 {
                        return Err(e);
                    }
                }
            }
        }
        Err(AppError::new(ErrorCode::EmbedFailed))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn missing_worker_is_embedder_missing() {
        let e = LocalEmbedder::new(PathBuf::from("/nonexistent/worker"), PathBuf::from("m"), 2);
        assert_eq!(
            e.embed_docs(&["a".into()]).unwrap_err().code,
            ErrorCode::EmbedderMissing
        );
    }

    #[test]
    fn worker_that_exits_is_a_clean_error() {
        let e = LocalEmbedder::new(PathBuf::from("/usr/bin/false"), PathBuf::from("m"), 2);
        assert_eq!(
            e.embed_docs(&["a".into()]).unwrap_err().code,
            ErrorCode::EmbedFailed
        );
    }

    /// The HTTP half (`call`) against a one-shot server; spawn/health are covered by the
    /// real-model integration test (`tests/real_embedder.rs`, ignored without the model).
    #[test]
    fn parses_openai_style_vectors() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 4096];
            let _ = s.read(&mut b);
            let body = r#"{"data":[{"index":1,"embedding":[0,1,5]},{"index":0,"embedding":[1,0,5]}]}"#;
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        let e = LocalEmbedder::new(PathBuf::from("x"), PathBuf::from("m"), 2);
        let v = e.call(port, &["a".into(), "b".into()]).unwrap();
        assert_eq!(v, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    }
}
