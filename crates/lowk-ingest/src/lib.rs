//! PDF/DOCX/TXT/MD to Markdown with page markers. M0 stub (M2 owns it).
use lowk_types::{AppError, Result};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestedDoc {
    pub md: String,
    pub pages: Option<u32>,
    pub warnings: Vec<String>,
}

/// No silent fallbacks: failures are errors, never empty documents.
pub fn convert(_path: &Path) -> Result<IngestedDoc> {
    Err(AppError::not_implemented())
}
