//! TXT / MD: encoding detection (UTF-8/16 BOM, valid UTF-8, otherwise a statistical guess so
//! Russian windows-1251 / KOI8-R / cp866 files read correctly), frontmatter kept as a fenced block.
use crate::md::{alnum_count, failed, sanitize, tidy};
use lowk_types::{AppError, ErrorCode, Result};
use std::path::Path;

/// Decodes raw bytes to text. Returns `(text, encoding name)`.
pub fn decode(bytes: &[u8]) -> Result<(String, &'static str)> {
    // NUL bytes in the first 8 KiB (outside UTF-16, handled by its BOM) mean a binary file.
    let utf16_bom = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    if !utf16_bom && bytes.iter().take(8192).any(|&b| b == 0) {
        return Err(AppError::new(ErrorCode::DocUnsupported)
            .with_param("reason", "binary")
            .with_detail("file contains NUL bytes, it is not a text file"));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok((String::from_utf8_lossy(rest).into_owned(), "utf-8"));
    }
    if let Some((enc, _)) = encoding_rs::Encoding::for_bom(bytes) {
        let (s, _, _) = enc.decode(bytes);
        return Ok((s.into_owned(), enc.name()));
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Ok((s.to_string(), "utf-8"));
    }
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    det.feed(bytes, true);
    let enc = det.guess(None, chardetng::Utf8Detection::Deny);
    let (s, _, _) = enc.decode(bytes);
    Ok((s.into_owned(), enc.name()))
}

/// Splits a leading `---` frontmatter block; returns `(frontmatter, body)`.
fn split_frontmatter(t: &str) -> (Option<&str>, &str) {
    let Some(rest) = t.strip_prefix("---\n") else {
        return (None, t);
    };
    match rest.find("\n---") {
        Some(i) => {
            let fm = &rest[..i];
            let after = &rest[i + 4..];
            match after
                .strip_prefix('\n')
                .or(if after.is_empty() { Some("") } else { None })
            {
                Some(body) => (Some(fm), body),
                None => (None, t),
            }
        }
        None => (None, t),
    }
}

pub fn convert(path: &Path, is_md: bool) -> Result<(String, &'static str)> {
    let bytes = std::fs::read(path)?;
    let (raw, _enc) = decode(&bytes)?;
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");
    let (fm, body) = if is_md {
        split_frontmatter(&text)
    } else {
        (None, text.as_str())
    };
    if alnum_count(body) == 0 && fm.is_none() {
        return Err(AppError::new(ErrorCode::DocEmpty).with_detail("the text file has no text"));
    }
    let mut md = String::new();
    if let Some(fm) = fm {
        md.push_str("```yaml\n");
        md.push_str(&sanitize(fm));
        md.push_str("\n```\n\n");
    }
    md.push_str(&sanitize(body));
    if alnum_count(&md) == 0 {
        return Err(failed("txt", "no text"));
    }
    Ok((tidy_keep_code(&md), if is_md { "md" } else { "txt" }))
}

/// Like `tidy` but leaves the user's own line structure alone (only trailing blank lines).
fn tidy_keep_code(s: &str) -> String {
    if s.contains("```") {
        format!("{}\n", s.trim_end())
    } else {
        tidy(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp1251_is_decoded() {
        let (enc, _, _) =
            encoding_rs::WINDOWS_1251.encode("Привет, мир! Это тестовый договор поставки товаров.");
        let (s, name) = decode(&enc).unwrap();
        assert!(s.contains("договор"), "{s} via {name}");
    }

    #[test]
    fn binary_rejected() {
        assert_eq!(decode(&[1, 2, 0, 3]).unwrap_err().code, ErrorCode::DocUnsupported);
    }

    #[test]
    fn frontmatter_kept() {
        let (f, b) = split_frontmatter("---\ntitle: x\n---\n# H\n");
        assert_eq!(f, Some("title: x"));
        assert_eq!(b, "# H\n");
    }
}
