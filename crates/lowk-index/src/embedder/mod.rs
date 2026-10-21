//! Embedders: the local EmbeddingGemma sidecar, an OpenAI-compatible cloud endpoint and a
//! deterministic fake for tests. `Off` mode is the absence of an embedder.
use lowk_types::Result;
use std::sync::Arc;

pub mod cloud;
pub mod fake;
pub mod local;
pub mod models;

pub use cloud::CloudEmbedder;
pub use fake::HashEmbedder;
pub use local::{find_worker, gemma_assets, gemma_info, LocalEmbedder, GEMMA_ENGINE_ID};

/// Text to vectors. Inputs arrive already formatted by `format_doc` / `format_query`, so
/// implementations are plain batch encoders. Output vectors need not be normalised.
pub trait Embedder: Send + Sync {
    /// Stable id stored with every vector; a change means a background re-embed.
    fn id(&self) -> &str;
    fn dim(&self) -> usize;
    /// True when text leaves the machine. Chunks of folders with `allow_cloud = false` are never
    /// embedded by a cloud embedder, and retrieval for such a folder never sends its query there.
    fn is_cloud(&self) -> bool {
        false
    }
    /// Model-specific document prefix (EmbeddingGemma: `title: T | text: X`).
    fn format_doc(&self, title: &str, text: &str) -> String {
        let t = if title.trim().is_empty() { "none" } else { title };
        format!("title: {t} | text: {text}")
    }
    fn format_query(&self, q: &str) -> String {
        format!("task: search result | query: {q}")
    }
    fn embed_docs(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        self.embed_docs(&[text.to_string()])?
            .pop()
            .ok_or_else(|| lowk_types::AppError::new(lowk_types::ErrorCode::EmbedFailed))
    }
}

pub type SharedEmbedder = Arc<dyn Embedder>;

/// rustls with the pure-Rust graviola provider (no C toolchain, no openssl).
pub(crate) fn tls_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls_graviola::default_provider())
}

/// Matryoshka truncation: keep the first `dim` values (the caller normalises).
pub(crate) fn truncate(mut v: Vec<f32>, dim: usize) -> Vec<f32> {
    v.truncate(dim);
    v
}
