//! OCR behind a trait. macOS: Apple Vision (VNRecognizeTextRequest, accurate). Windows:
//! `Windows.Media.Ocr` (languages come from the installed language packs). Elsewhere (Linux CI):
//! no engine, so image/scanned inputs fail with a typed error instead of producing empty text.
//! Tests inject [`StubOcr`].
use lowk_types::{AppError, ErrorCode, Result};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct OcrOutput {
    pub text: String,
    /// Mean line confidence 0..1 when the engine reports it.
    pub confidence: Option<f32>,
}

pub trait Ocr: Send + Sync {
    /// Whether this engine can run on this machine (language packs, OS version).
    fn available(&self) -> bool;
    /// Short engine name for the `converter` field, e.g. `vision`.
    fn name(&self) -> &'static str;
    fn recognize_image(&self, path: &Path, languages: &[String]) -> Result<OcrOutput>;
    /// Renders page `page_index` (0-based) of a PDF and recognizes it.
    fn recognize_pdf_page(&self, pdf: &Path, page_index: usize, languages: &[String]) -> Result<OcrOutput>;
}

/// No engine on this platform.
pub struct NoOcr;
impl Ocr for NoOcr {
    fn available(&self) -> bool {
        false
    }
    fn name(&self) -> &'static str {
        "none"
    }
    fn recognize_image(&self, _: &Path, _: &[String]) -> Result<OcrOutput> {
        Err(unavailable())
    }
    fn recognize_pdf_page(&self, _: &Path, _: usize, _: &[String]) -> Result<OcrOutput> {
        Err(unavailable())
    }
}

pub(crate) fn unavailable() -> AppError {
    AppError::new(ErrorCode::OcrFailed)
        .with_param("reason", "unavailable")
        .with_detail("no OCR engine on this platform")
}

/// The engine of the current platform.
pub fn platform_ocr() -> Box<dyn Ocr> {
    #[cfg(target_os = "macos")]
    {
        Box::new(crate::ocr_macos::Vision)
    }
    #[cfg(windows)]
    {
        Box::new(crate::ocr_windows::WinOcr)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Box::new(NoOcr)
    }
}

/// `ru` / `en-US` / `ru_RU` to a BCP 47 tag with region, as Vision wants (`ru-RU`).
pub(crate) fn bcp47(tag: &str) -> String {
    let t = tag.replace('_', "-");
    if t.contains('-') {
        return t;
    }
    match t.to_ascii_lowercase().as_str() {
        "ru" => "ru-RU",
        "en" => "en-US",
        "uk" => "uk-UA",
        "de" => "de-DE",
        "fr" => "fr-FR",
        "es" => "es-ES",
        "it" => "it-IT",
        "pt" => "pt-BR",
        other => return other.to_string(),
    }
    .to_string()
}

/// Languages to ask the engine for: the request's, else Russian + English (the product's pair).
pub(crate) fn languages_or_default(langs: &[String]) -> Vec<String> {
    if langs.is_empty() {
        vec!["ru-RU".into(), "en-US".into()]
    } else {
        let mut v: Vec<String> = langs.iter().map(|l| bcp47(l)).collect();
        for extra in ["en-US", "ru-RU"] {
            if !v.iter().any(|x| x == extra) {
                v.push(extra.into());
            }
        }
        v
    }
}

/// Deterministic OCR for tests: returns `text` for every image/page, or fails.
pub struct StubOcr {
    pub text: String,
    pub confidence: Option<f32>,
    pub fail: bool,
}

impl StubOcr {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.into(),
            confidence: Some(0.95),
            fail: false,
        }
    }
}

impl Ocr for StubOcr {
    fn available(&self) -> bool {
        true
    }
    fn name(&self) -> &'static str {
        "stub"
    }
    fn recognize_image(&self, _: &Path, _: &[String]) -> Result<OcrOutput> {
        if self.fail {
            return Err(AppError::new(ErrorCode::OcrFailed).with_detail("stub failure"));
        }
        Ok(OcrOutput {
            text: self.text.clone(),
            confidence: self.confidence,
        })
    }
    fn recognize_pdf_page(&self, _: &Path, page: usize, _: &[String]) -> Result<OcrOutput> {
        if self.fail {
            return Err(AppError::new(ErrorCode::OcrFailed).with_detail("stub failure"));
        }
        Ok(OcrOutput {
            text: format!("{} (page {})", self.text, page + 1),
            confidence: self.confidence,
        })
    }
}
