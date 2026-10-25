//! Contract: search, retrieval, sources, embedder. Implemented by `lowk-index`.
use crate::{AppError, ChunkKind, FolderRef, Result, Ulid};
use serde::{Deserialize, Serialize};
use specta::Type;

/// What a source label points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Call,
    Doc,
    /// Screenshot of a call (OCR/caption text is what was indexed).
    Image,
    /// The folder's own instructions.
    Instructions,
}

/// A clickable origin of a chunk: «Spec.pdf, p. 12» or «Weekly sync, 12:45».
/// Clicking opens the call at `at_s`, or the document at `page` / `heading_path`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SourceRef {
    pub kind: RefKind,
    /// Call id (`Call`, `Image`), document id (`Doc`) or folder id (`Instructions`).
    pub id: Ulid,
    pub folder: FolderRef,
    pub title: String,
    /// Offset into the call, seconds.
    pub at_s: Option<u32>,
    pub page: Option<u32>,
    pub heading_path: Vec<String>,
    pub chunk_id: Option<String>,
    /// Came from the folder's pinned documents rather than from search.
    pub pinned: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// FTS5 + vectors fused (falls back to `Keyword` while the embedder is not ready; the
    /// result says so in `SearchResults::mode_used`, the UI shows it).
    #[default]
    Hybrid,
    Keyword,
    Semantic,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct SearchFilters {
    pub folders: Vec<FolderRef>,
    /// RFC 3339 dates (inclusive); apply to call start / document added.
    pub from: Option<String>,
    pub to: Option<String>,
    /// Empty = everything (speech, docs, screenshots); `Qa` is down-weighted, never excluded.
    pub kinds: Vec<ChunkKind>,
    pub include_trash: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SearchQuery {
    pub text: String,
    pub filters: SearchFilters,
    pub mode: SearchMode,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SearchHit {
    pub chunk_id: String,
    pub kind: ChunkKind,
    pub source: SourceRef,
    pub folder_title: String,
    /// Plain snippet; `highlights` are (start, end) UTF-16 offsets into it, so the UI can mark
    /// matches without parsing markup.
    pub snippet: String,
    pub highlights: Vec<(u32, u32)>,
    pub tags: Vec<String>,
    /// RFC 3339: call start or document added.
    pub when: Option<String>,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FacetCount {
    pub folder: FolderRef,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub total: u32,
    pub took_ms: u32,
    pub mode_used: SearchMode,
    pub facets: Vec<FacetCount>,
}

/// State of the semantic (vector) half of the index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SemanticState {
    /// Embedder mode is `off`.
    Off,
    /// Model files are not downloaded yet.
    ModelMissing,
    /// Embedding existing chunks in the background; keyword search works meanwhile.
    Building {
        done: u32,
        total: u32,
    },
    Ready,
    Failed {
        error: AppError,
    },
}

/// `index://status` payload and `index_status` result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct IndexStatus {
    pub files: u32,
    pub chunks: u32,
    /// Files waiting to be (re)indexed.
    pub pending: u32,
    pub semantic: SemanticState,
    pub embedder_model: Option<String>,
    pub last_error: Option<AppError>,
}

/// Which embedder is installed / downloading (`embedder_info`). Download progress arrives as
/// `engine://progress` with `engine == "embedder"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EmbedderInfo {
    pub id: String,
    pub name: String,
    pub dim: u32,
    pub size_mb: u32,
    pub installed: bool,
    pub installing: bool,
    pub license: String,
    pub license_url: String,
}

/// What `Retriever::retrieve_pack` returns for the Ask flow, already deduplicated and packed
/// into the budgets of the query.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct ContextPack {
    /// Folder instructions (always included, first turn only; the session decides).
    pub instructions: Option<crate::ContextChunk>,
    /// Pinned documents of the folder, within `pinned_budget_tokens`.
    pub pinned: Vec<crate::ContextChunk>,
    /// Relevant chunks from documents and previous calls of the folder.
    pub relevant: Vec<crate::ContextChunk>,
    /// Chunks left out because the folder has `allow_cloud = false` (overlay shows a badge).
    pub withheld: u32,
    /// Pinned documents that did not fit the budget.
    pub pinned_dropped: u32,
}

/// What to (re)index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ReindexScope {
    All,
    /// A call, a document or a folder.
    Item(Ulid),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IndexItemKind {
    Call,
    Doc,
}

/// What the app hands to the index for one call or document (the index never reads the vault:
/// feature crates do not depend on each other). Chunking happens inside `lowk-index`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct IndexItem {
    pub id: Ulid,
    pub kind: IndexItemKind,
    pub folder: FolderRef,
    pub title: String,
    pub tags: Vec<String>,
    /// RFC 3339: call start / document added.
    pub when: Option<String>,
    /// Markdown body without frontmatter (`<!-- p:N -->` markers, `lowk-qa` / `lowk-image` callouts).
    pub md: String,
    /// blake3 of the whole file; unchanged hash = skip.
    pub content_hash: String,
    pub pinned: bool,
    pub deleted: bool,
}

/// Folder facts the index needs for retrieval (policy, instructions, boosts).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct IndexFolder {
    pub folder: FolderRef,
    pub title: String,
    pub allow_cloud: bool,
    pub instructions: String,
    pub tags: Vec<String>,
}

/// Implemented by `lowk-index`. All methods are blocking and may be called from any thread.
pub trait SearchIndex: Send + Sync {
    fn search(&self, q: &SearchQuery) -> Result<SearchResults>;
    fn status(&self) -> IndexStatus;
    /// Queues a (re)index; returns immediately. Progress arrives as `IndexStatus` events.
    fn reindex(&self, scope: ReindexScope) -> Result<()>;
    /// Chunks and (when the embedder is ready) embeds one item; skips when the hash is unchanged.
    fn upsert(&self, item: &IndexItem) -> Result<()>;
    fn upsert_folder(&self, folder: &IndexFolder) -> Result<()>;
    /// `(id, content_hash)` of everything indexed: the start-up reconcile diffs it against the vault.
    fn known(&self) -> Result<Vec<(Ulid, String)>>;
    /// Marks everything of `id` deleted (trash) or removes it (`purge`).
    fn remove(&self, id: &Ulid, purge: bool) -> Result<()>;
}
