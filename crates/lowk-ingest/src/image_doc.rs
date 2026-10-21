//! Images (PNG/JPEG/HEIC/WebP/TIFF): OCR text plus the pixel size. No OCR engine or OCR switched
//! off is an error (`OcrFailed`), an image with no recognizable text is `DocEmpty`.
use crate::md::{alnum_count, sanitize, tidy, warn};
use crate::ocr::{languages_or_default, Ocr};
use crate::Progress;
use lowk_types::{AppError, ErrorCode, IngestRequest, IngestWarning, IngestWarningCode, Result};
use std::path::Path;

pub struct ImageOut {
    pub md: String,
    pub warnings: Vec<IngestWarning>,
}

pub fn convert(
    path: &Path,
    req: &IngestRequest,
    ocr: &dyn Ocr,
    progress: &mut Progress<'_>,
) -> Result<ImageOut> {
    if !req.ocr {
        return Err(AppError::new(ErrorCode::OcrFailed)
            .with_param("reason", "off")
            .with_detail("OCR is switched off in settings, an image can only be read through OCR"));
    }
    if !ocr.available() {
        return Err(crate::ocr::unavailable());
    }
    progress.report(0, 1)?;
    // pixel size when the `image` crate can read the header (HEIC cannot; that is fine)
    let dims = image::ImageReader::open(path)
        .ok()
        .and_then(|r| r.with_guessed_format().ok())
        .and_then(|r| r.into_dimensions().ok());
    let out = ocr.recognize_image(path, &languages_or_default(&req.languages))?;
    progress.report(1, 1)?;
    let text = sanitize(&out.text);
    if alnum_count(&text) == 0 {
        return Err(AppError::new(ErrorCode::DocEmpty)
            .with_param("reason", "no_text_in_image")
            .with_detail("OCR found no text in the image"));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let size = dims.map(|(w, h)| format!(", {w}x{h} px")).unwrap_or_default();
    let mut warnings = vec![warn(IngestWarningCode::OcrUsed, None, None)];
    if out.confidence.is_some_and(|c| c < 0.5) {
        warnings.push(warn(
            IngestWarningCode::OcrLowConfidence,
            None,
            out.confidence.map(|c| format!("{:.0}%", c * 100.0)),
        ));
    }
    Ok(ImageOut {
        md: tidy(&format!("*Image: {}{size}*\n\n{}", sanitize(&name), text.trim())),
        warnings,
    })
}
