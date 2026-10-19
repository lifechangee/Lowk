//! SQLite index (FTS5 + vectors), chunker, hybrid search. M0 stub (M2 owns it).
use lowk_types::{ContextChunk, Result};

pub use lowk_types::fakes::NoopRetriever;

pub trait Embedder: Send + Sync {
    fn dim(&self) -> usize;
    fn embed_docs(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn embed_query(&self, text: &str) -> Result<Vec<f32>>;
}

pub trait VectorIndex: Send + Sync {
    fn upsert(&mut self, chunk_id: &str, vector: &[f32]) -> Result<()>;
    fn search(&self, query: &[f32], k: usize) -> Result<Vec<(String, f32)>>;
}

/// Convenience alias used by the future `Retriever` implementation.
pub type Chunks = Vec<ContextChunk>;
