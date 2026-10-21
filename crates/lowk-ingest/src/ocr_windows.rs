//! Windows OCR: `Windows.Media.Ocr` (engine per installed language pack) and `Windows.Data.Pdf`
//! for rasterising PDF pages. Verified by compilation in CI until a Windows machine is available
//! (docs/M2-PLAN.md risks); the WinRT calls are the documented ones and are blocking (`.join()`),
//! which is fine: conversion runs on a worker thread.
use crate::ocr::{languages_or_default, Ocr, OcrOutput};
use lowk_types::{AppError, ErrorCode, Result};
use std::path::Path;
use windows::core::{Interface, HSTRING};
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::InMemoryRandomAccessStream;
use windows::Storage::{FileAccessMode, StorageFile};

pub struct WinOcr;

fn ocr_err(detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::OcrFailed).with_detail(detail)
}

fn werr(e: windows::core::Error) -> AppError {
    ocr_err(e.message())
}

/// First language with an installed OCR pack, else the user-profile languages.
fn engine(languages: &[String]) -> Result<OcrEngine> {
    for l in languages_or_default(languages) {
        if let Ok(lang) = Language::CreateLanguage(&HSTRING::from(l.as_str())) {
            if OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
                if let Ok(e) = OcrEngine::TryCreateFromLanguage(&lang) {
                    return Ok(e);
                }
            }
        }
    }
    OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| {
        AppError::new(ErrorCode::OcrFailed)
            .with_param("reason", "no_language_pack")
            .with_detail("no OCR language pack is installed (Settings > Time & language > Language)")
    })
}

fn recognize(engine: &OcrEngine, bmp: &SoftwareBitmap) -> Result<OcrOutput> {
    let max = OcrEngine::MaxImageDimension().unwrap_or(10000) as i32;
    if bmp.PixelWidth().map_err(werr)? > max || bmp.PixelHeight().map_err(werr)? > max {
        return Err(ocr_err("image is larger than the OCR engine allows"));
    }
    let res = engine.RecognizeAsync(bmp).map_err(werr)?.join().map_err(werr)?;
    let lines = res.Lines().map_err(werr)?;
    let mut text = Vec::new();
    for l in lines {
        text.push(l.Text().map_err(werr)?.to_string());
    }
    // Windows.Media.Ocr reports no confidence.
    Ok(OcrOutput {
        text: text.join("\n"),
        confidence: None,
    })
}

fn bitmap_from_stream(stream: &windows::Storage::Streams::IRandomAccessStream) -> Result<SoftwareBitmap> {
    let decoder = BitmapDecoder::CreateAsync(stream)
        .map_err(werr)?
        .join()
        .map_err(werr)?;
    decoder
        .GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied)
        .map_err(werr)?
        .join()
        .map_err(werr)
}

fn storage_file(path: &Path) -> Result<StorageFile> {
    let abs = std::fs::canonicalize(path)?;
    // canonicalize yields a `\\?\` path, which StorageFile rejects
    let s = abs.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    StorageFile::GetFileFromPathAsync(&HSTRING::from(s))
        .map_err(werr)?
        .join()
        .map_err(werr)
}

impl Ocr for WinOcr {
    fn available(&self) -> bool {
        OcrEngine::TryCreateFromUserProfileLanguages().is_ok()
    }
    fn name(&self) -> &'static str {
        "windows-ocr"
    }

    fn recognize_image(&self, path: &Path, languages: &[String]) -> Result<OcrOutput> {
        let eng = engine(languages)?;
        let file = storage_file(path)?;
        let stream = file
            .OpenAsync(FileAccessMode::Read)
            .map_err(werr)?
            .join()
            .map_err(werr)?;
        let bmp = bitmap_from_stream(&stream.cast().map_err(werr)?)?;
        recognize(&eng, &bmp)
    }

    fn recognize_pdf_page(&self, pdf: &Path, page_index: usize, languages: &[String]) -> Result<OcrOutput> {
        let eng = engine(languages)?;
        let file = storage_file(pdf)?;
        let doc = PdfDocument::LoadFromFileAsync(&file)
            .map_err(werr)?
            .join()
            .map_err(|e| {
                // Windows.Data.Pdf reports password protected files as a load error
                AppError::new(ErrorCode::OcrFailed).with_detail(e.message())
            })?;
        if doc.IsPasswordProtected().unwrap_or(false) {
            return Err(AppError::new(ErrorCode::DocEncrypted));
        }
        let page = doc.GetPage(page_index as u32).map_err(werr)?;
        let size = page.Size().map_err(werr)?;
        let long = size.Width.max(size.Height).max(1.0);
        let scale = (2200.0f32 / long).clamp(1.0, 4096.0 / long);
        let opts = PdfPageRenderOptions::new().map_err(werr)?;
        opts.SetDestinationWidth((size.Width * scale) as u32)
            .map_err(werr)?;
        opts.SetDestinationHeight((size.Height * scale) as u32)
            .map_err(werr)?;
        let stream = InMemoryRandomAccessStream::new().map_err(werr)?;
        page.RenderWithOptionsToStreamAsync(&stream, &opts)
            .map_err(werr)?
            .join()
            .map_err(werr)?;
        let bmp = bitmap_from_stream(&stream.cast().map_err(werr)?)?;
        recognize(&eng, &bmp)
    }
}
