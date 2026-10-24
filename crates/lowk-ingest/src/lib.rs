//! Lowk document ingest: a pure function of one file to Markdown with
//! `<!-- p:N -->` markers (page / slide / sheet). Implements [`lowk_types::DocIngestor`].
//!
//! Rules this crate keeps:
//! * No silent fallbacks: a file either converts, or fails with a typed `AppError`
//!   (`DocUnsupported`, `DocEncrypted`, `DocTooLarge`, `DocEmpty`, `OcrFailed`, `IngestFailed`,
//!   `IngestCancelled`); findings that do not fail the file are `IngestWarning`s.
//! * Never an empty document: no text at all is `DocEmpty`.
//! * Conversion is page/slide/sheet at a time with progress and cancellation between units.
//! * Malformed input must not panic; every converter runs behind `catch_unwind` as a last line of
//!   defence (release builds use `panic = "abort"`, so parsers also return errors on their own).
//! * OCR is a trait ([`Ocr`]): Apple Vision on macOS, `Windows.Media.Ocr` on Windows, a stub in tests.
//!
//! `converter` values: `pdf-text`, `pdf-ocr` (some pages OCR'd), `keynote-pdf`, `docx`, `pptx`,
//! `xlsx`, `csv`, `txt`, `md`, `image-ocr`. The store derives the frontmatter `unit`
//! (`page` | `slide` | `sheet`) from it with [`unit_of`].
mod docx;
mod image_doc;
mod keynote;
mod md;
pub mod ocr;
#[cfg(target_os = "macos")]
mod ocr_macos;
pub mod ocr_text;
#[cfg(windows)]
mod ocr_windows;
mod pdf;
mod pptx;
mod sheet;
mod text;
mod xml;

pub use ocr::{platform_ocr, NoOcr, Ocr, OcrOutput, StubOcr};
pub use ocr_text::reflow as reflow_ocr_text;

use lowk_types::{
    AppError, DocIngestor, DocKind, ErrorCode, IngestRequest, IngestWarningCode, IngestedDoc, Result,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::Arc;

/// Conversion limits (the app maps `Settings.docs` onto these).
#[derive(Clone, Debug)]
pub struct Limits {
    /// Larger files fail with `DocTooLarge`.
    pub max_bytes: u64,
    /// Larger files convert but carry a `LargeFile` warning.
    pub warn_bytes: u64,
    /// Rows kept per sheet / CSV; more gives `SheetTruncated`.
    pub max_rows: usize,
    pub max_cols: usize,
    /// Data rows per table chunk in big sheets (header repeated per chunk).
    pub chunk_rows: usize,
    /// Largest uncompressed XML part of an Office file (zip bomb guard).
    pub max_xml_bytes: u64,
    /// A PDF page with fewer letters/digits than this is treated as scanned.
    pub scanned_min_chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 500 * 1024 * 1024,
            warn_bytes: 25 * 1024 * 1024,
            max_rows: 100_000,
            max_cols: 200,
            chunk_rows: 200,
            max_xml_bytes: 256 * 1024 * 1024,
            scanned_min_chars: 20,
        }
    }
}

impl Limits {
    /// From `Settings.docs` values in megabytes.
    pub fn from_mb(warn_slow_mb: u32, max_file_mb: u32) -> Self {
        Self {
            warn_bytes: u64::from(warn_slow_mb) * 1024 * 1024,
            max_bytes: u64::from(max_file_mb) * 1024 * 1024,
            ..Self::default()
        }
    }
}

/// Progress + cancellation in one place: `report` is the only way converters advance.
pub struct Progress<'a> {
    cb: &'a mut dyn FnMut(u32, u32),
    cont: &'a dyn Fn() -> bool,
}

impl<'a> Progress<'a> {
    pub fn new(cb: &'a mut dyn FnMut(u32, u32), cont: &'a dyn Fn() -> bool) -> Self {
        Self { cb, cont }
    }
    /// Checks for cancellation, then reports.
    pub fn report(&mut self, done: u32, total: u32) -> Result<()> {
        self.check()?;
        (self.cb)(done, total);
        Ok(())
    }
    pub fn check(&self) -> Result<()> {
        if (self.cont)() {
            Ok(())
        } else {
            Err(AppError::new(ErrorCode::IngestCancelled))
        }
    }
}

/// `page` | `slide` | `sheet`: what the `<!-- p:N -->` markers count, for the frontmatter.
pub fn unit_of(converter: &str) -> &'static str {
    match converter {
        "pptx" | "keynote-pdf" => "slide",
        "xlsx" | "csv" => "sheet",
        _ => "page",
    }
}

pub struct Ingestor {
    ocr: Arc<dyn Ocr>,
    limits: Limits,
}

impl Default for Ingestor {
    fn default() -> Self {
        Self::new()
    }
}

impl Ingestor {
    /// Platform OCR, default limits.
    pub fn new() -> Self {
        Self {
            ocr: Arc::from(platform_ocr()),
            limits: Limits::default(),
        }
    }
    pub fn with_ocr(ocr: Arc<dyn Ocr>) -> Self {
        Self {
            ocr,
            limits: Limits::default(),
        }
    }
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    pub fn ocr_available(&self) -> bool {
        self.ocr.available()
    }

    fn run(&self, req: &IngestRequest, path: &Path, progress: &mut Progress<'_>) -> Result<IngestedDoc> {
        let lim = &self.limits;
        let ok = |md: String, pages: Option<u32>, warnings, converter: &str| IngestedDoc {
            md,
            pages,
            warnings,
            converter: converter.into(),
            assets: vec![],
        };
        match req.kind {
            DocKind::Pdf => {
                let r = pdf::convert(path, req, &*self.ocr, lim, progress)?;
                Ok(ok(r.md, Some(r.pages), r.warnings, r.converter))
            }
            DocKind::Keynote => {
                let r = keynote::convert(path, req, &*self.ocr, lim, progress)?;
                Ok(ok(r.md, Some(r.pages), r.warnings, "keynote-pdf"))
            }
            DocKind::Docx => {
                let r = docx::convert(path, lim)?;
                Ok(ok(r.md, r.pages, r.warnings, "docx"))
            }
            DocKind::Pptx => {
                let r = pptx::convert(path, lim, progress)?;
                Ok(ok(r.md, Some(r.slides), r.warnings, "pptx"))
            }
            DocKind::Xlsx => {
                let r = sheet::convert_xlsx(path, lim, progress)?;
                Ok(ok(r.md, Some(r.units), r.warnings, "xlsx"))
            }
            DocKind::Csv => {
                let r = sheet::convert_csv(path, lim, progress)?;
                Ok(ok(r.md, Some(r.units), r.warnings, "csv"))
            }
            DocKind::Txt | DocKind::Md => {
                progress.report(0, 1)?;
                let (md, conv) = text::convert(path, req.kind == DocKind::Md)?;
                progress.report(1, 1)?;
                Ok(ok(md, None, vec![], conv))
            }
            DocKind::Image => {
                let r = image_doc::convert(path, req, &*self.ocr, progress)?;
                Ok(ok(r.md, None, r.warnings, "image-ocr"))
            }
        }
    }
}

impl DocIngestor for Ingestor {
    fn supports(&self, _kind: DocKind) -> bool {
        // Every kind is supported; Keynote and OCR-dependent inputs fail with a typed error where
        // the platform lacks the means (never silently).
        true
    }

    fn convert(
        &self,
        req: &IngestRequest,
        progress: &mut dyn FnMut(u32, u32),
        should_continue: &dyn Fn() -> bool,
    ) -> Result<IngestedDoc> {
        let path = Path::new(&req.source_path);
        let meta = std::fs::metadata(path).map_err(|e| {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                ErrorCode::NotFound
            } else {
                ErrorCode::Io
            };
            AppError::new(code).with_detail(format!("{}: {e}", path.display()))
        })?;
        if !meta.is_file() {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_detail("not a regular file"));
        }
        let size = meta.len();
        if size == 0 {
            return Err(AppError::new(ErrorCode::DocEmpty).with_detail("the file is empty (0 bytes)"));
        }
        if size > self.limits.max_bytes {
            return Err(AppError::new(ErrorCode::DocTooLarge)
                .with_param("size_mb", size / (1024 * 1024))
                .with_param("max_mb", self.limits.max_bytes / (1024 * 1024)));
        }
        let mut prog = Progress::new(progress, should_continue);
        prog.check()?;
        let mut out =
            catch_unwind(AssertUnwindSafe(|| self.run(req, path, &mut prog))).unwrap_or_else(|p| {
                let msg = p
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                tracing::error!("converter panicked on {:?}: {msg}", req.kind);
                Err(md::failed("internal", format!("converter panicked: {msg}")))
            })?;
        if md::alnum_count(&out.md) == 0 {
            return Err(AppError::new(ErrorCode::DocEmpty).with_detail("conversion produced no text"));
        }
        if size > self.limits.warn_bytes {
            out.warnings.push(md::warn(
                IngestWarningCode::LargeFile,
                None,
                Some(format!("{} MB", size / (1024 * 1024))),
            ));
        }
        Ok(out)
    }
}
