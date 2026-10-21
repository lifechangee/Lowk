//! Lowk search index (WP-C): SQLite FTS5 over stemmed RU+EN text, flat int8 vectors from the
//! embedder, RRF hybrid ranking, filters, and the `Retriever` for Ask. The index is a
//! rebuildable projection of the vault: it never reads the vault, the app feeds it `IndexItem`s.
pub mod chunker;
mod db;
pub mod embedder;
mod index;
mod retrieve;
mod search;
pub mod text;
pub mod vector;

pub use embedder::{Embedder, SharedEmbedder};
pub use index::{Index, StatusFn};
pub use lowk_types::fakes::NoopRetriever;
pub use retrieve::DEFAULT_PINNED_BUDGET;
pub use vector::VectorIndex;
