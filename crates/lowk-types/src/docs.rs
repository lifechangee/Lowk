//! Contract: documents, ingest, pinned docs. Implemented by `lowk-ingest` (conversion),
//! `lowk-store` (files on disk), `lowk-app` (wiring).
use crate::{AppError, DocId, FolderRef, LangTag, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

/// What a source file is. Presentations and spreadsheets are first-class kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    Pdf,
    Docx,
    /// PowerPoint (`.pptx`).
    Pptx,
    /// Apple Keynote (`.key`), converted through PDF.
    Keynote,
    Xlsx,
    Csv,
    Txt,
    Md,
    /// PNG/JPG/HEIC/... run through OCR.
    Image,
}

impl DocKind {
    /// Maps a lower-case file extension (no dot) to a kind; `None` = unsupported.
    pub fn from_extension(ext: &str) -> Option<Self> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "pdf" => Self::Pdf,
            "docx" => Self::Docx,
            "pptx" => Self::Pptx,
            "key" => Self::Keynote,
            "xlsx" => Self::Xlsx,
            "csv" | "tsv" => Self::Csv,
            "txt" | "text" => Self::Txt,
            "md" | "markdown" => Self::Md,
            "png" | "jpg" | "jpeg" | "heic" | "webp" | "tif" | "tiff" => Self::Image,
            _ => return None,
        })
    }
}

/// Lifecycle of an imported document. `Failed` is always shown with its error
/// (no silent fallbacks, no empty documents).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IngestStatus {
    Queued,
    Converting,
    /// Converted; chunks are being (re)indexed.
    Indexing,
    Ready,
    Failed,
    Cancelled,
}

/// Non-fatal conversion findings, shown next to the document (UI translates the code).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IngestWarningCode {
    /// Scanned PDF: text comes from OCR.
    OcrUsed,
    OcrLowConfidence,
    /// Some pages could not be read and are marked as missing in the Markdown.
    PagesSkipped,
    TablesFlattened,
    /// Slides: only text and speaker notes were taken; pictures were not read.
    SlideImagesIgnored,
    /// Spreadsheet cut at the row/column limits.
    SheetTruncated,
    EncryptedPartial,
    LargeFile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct IngestWarning {
    pub code: IngestWarningCode,
    pub page: Option<u32>,
    pub detail: Option<String>,
}

/// A document of a folder (`Docs/<name>.md` next to the untouched original).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Document {
    pub id: DocId,
    pub folder: FolderRef,
    pub title: String,
    pub kind: DocKind,
    /// Original file name as imported.
    pub file_name: String,
    /// Paths relative to the vault root.
    pub md_path: String,
    pub original_path: String,
    pub size_bytes: u64,
    pub pages: Option<u32>,
    pub status: IngestStatus,
    pub error: Option<AppError>,
    pub warnings: Vec<IngestWarning>,
    /// Always sent to the model for calls of this folder (within the pinned budget).
    pub pinned: bool,
    /// Rough token count of the converted text (for the pinned budget meter).
    pub tokens: u32,
    pub tags: Vec<String>,
    /// RFC 3339 with offset.
    pub added: String,
    pub source_sha256: String,
    /// `converter` frontmatter value, e.g. `pdf-text`, `pdf-ocr`, `docx`.
    pub converter: String,
    pub deleted: bool,
}

/// One file to import into a folder (`doc_import`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NewDocument {
    pub folder: FolderRef,
    /// Absolute path on disk (drag-and-drop or the file dialog).
    pub source_path: String,
    pub title: Option<String>,
    pub pin: bool,
    /// What to do when the same content (SHA-256 of the original) is already in the folder.
    #[serde(default)]
    pub on_duplicate: DupAction,
}

/// Policy for an import whose content is already in the target folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DupAction {
    /// Refuse with `ErrorCode::DocDuplicate` (params `id`, `title` of the existing document).
    #[default]
    Reject,
    /// Import it as one more document.
    Copy,
    /// Import it and move the existing document to the trash (undo restores it).
    Replace,
}

/// A file of an import batch that is already in its folder (`doc_find_duplicates`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct DuplicateHit {
    /// Index into the checked `NewDocument` list.
    pub index: u32,
    pub existing: Document,
}

/// Editable document fields (`doc_patch`); `None` = unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct DocPatch {
    pub title: Option<String>,
    pub pinned: Option<bool>,
    pub folder: Option<FolderRef>,
}

/// Input of `DocIngestor::convert` (pure function of one file).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct IngestRequest {
    pub doc: DocId,
    pub source_path: String,
    pub kind: DocKind,
    /// OCR languages (call languages / settings); empty = engine default.
    pub languages: Vec<LangTag>,
    pub ocr: bool,
}

/// Output of conversion: Markdown with `<!-- p:12 -->` page (or slide / sheet) markers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct IngestedDoc {
    pub md: String,
    pub pages: Option<u32>,
    pub warnings: Vec<IngestWarning>,
    pub converter: String,
    /// Images extracted next to the Markdown (`<name>.assets/...`), relative names.
    pub assets: Vec<String>,
}

/// `doc://progress` payload (also used for the final `Ready` / `Failed`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct IngestProgress {
    pub doc: DocId,
    pub folder: FolderRef,
    pub status: IngestStatus,
    /// Pages / slides / sheets done and total; 0 total = unknown.
    pub done: u32,
    pub total: u32,
    pub eta_s: Option<u32>,
    pub error: Option<AppError>,
}

/// Pinned-documents budget meter of a folder (`docs_pinned_usage`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PinnedUsage {
    pub count: u32,
    pub tokens: u32,
    pub budget_tokens: u32,
    pub over_budget: bool,
}

/// Implemented by `lowk-ingest`. A pure function of one file: no vault access, no network.
/// Errors are `ErrorCode::IngestFailed` with params; never an empty document.
pub trait DocIngestor: Send + Sync {
    fn supports(&self, kind: DocKind) -> bool;
    /// `progress(done, total)` is called per page/slide/sheet; cancel by returning `false` from
    /// `should_continue`.
    fn convert(
        &self,
        req: &IngestRequest,
        progress: &mut dyn FnMut(u32, u32),
        should_continue: &dyn Fn() -> bool,
    ) -> Result<IngestedDoc>;
}

/// Implemented by `lowk-store`: documents on disk (`Docs/`), `type: doc` frontmatter, trash.
pub trait DocumentStore: Send + Sync {
    fn list_docs(&self, folder: &FolderRef) -> Result<Vec<Document>>;
    fn get_doc(&self, id: &DocId) -> Result<Document>;
    /// Copies the original into the folder and writes the placeholder `.md` (`status: queued`).
    fn add_doc(&self, n: &NewDocument) -> Result<Document>;
    fn set_status(&self, id: &DocId, status: IngestStatus, error: Option<AppError>) -> Result<Document>;
    /// Writes the converted Markdown + assets and marks the document `Indexing`.
    fn save_converted(&self, id: &DocId, doc: &IngestedDoc) -> Result<Document>;
    fn read_md(&self, id: &DocId) -> Result<String>;
    /// User edit in the Markdown editor (re-index follows).
    fn save_md(&self, id: &DocId, md: &str) -> Result<Document>;
    fn patch_doc(&self, id: &DocId, p: &DocPatch) -> Result<Document>;
    /// Moves to the Trash (restorable); never hard-deletes.
    fn delete_doc(&self, id: &DocId) -> Result<()>;
    fn restore_doc(&self, id: &DocId) -> Result<Document>;
}
