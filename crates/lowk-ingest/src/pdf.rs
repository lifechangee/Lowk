//! PDF to Markdown. Per page: the text layer through `pdf_oxide` (`extract_text` for the text, which
//! keeps punctuation in mixed-font Cyrillic runs where its Markdown converter does not; headings
//! from span font sizes relative to the document's body size),
//! pages without usable text (scans, broken font maps) through OCR, then document-level cleanup:
//! repeating headers/footers and page numbers out, hyphenation joined, wrapped lines re-flowed.
//! Pages that cannot be read are marked in the Markdown and reported as `PagesSkipped`.
use crate::md::{alnum_count, failed, marker, sanitize, tidy, warn};
use crate::ocr::{languages_or_default, Ocr};
use crate::{Limits, Progress};
use lowk_types::{AppError, ErrorCode, IngestRequest, IngestWarning, IngestWarningCode, Result};
use pdf_oxide::document::PdfDocument;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

pub struct PdfOut {
    pub md: String,
    pub pages: u32,
    pub warnings: Vec<IngestWarning>,
    pub converter: &'static str,
}

/// One text-layer line with the font size of its glyphs (0 = unknown / blank).
#[derive(Clone)]
struct Ln {
    t: String,
    size: f32,
}

enum Page {
    Text(Vec<Ln>),
    Ocr(String),
    Missing(String),
}

fn norm_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text of a page plus a font size per line, matched from the spans that make the line up.
fn read_page(doc: &PdfDocument, i: usize) -> std::result::Result<Vec<Ln>, String> {
    let text = doc.extract_text(i).map_err(|e| e.to_string())?;
    let spans = doc.extract_spans(i).unwrap_or_default();
    let spans: Vec<(String, f32)> = spans
        .iter()
        .map(|s| (norm_ws(&s.text), s.font_size))
        .filter(|(t, _)| t.chars().count() >= 2)
        .collect();
    let mut out = vec![];
    for line in text.lines() {
        let t = sanitize(line.trim());
        if t.is_empty() {
            out.push(Ln { t, size: 0.0 });
            continue;
        }
        let nl = norm_ws(&t);
        // character-weighted mode of the sizes of spans contained in this line
        let mut w: Vec<(f32, usize)> = vec![];
        for (st, sz) in &spans {
            if nl.contains(st.as_str()) {
                match w.iter_mut().find(|(s, _)| (s - sz).abs() < 0.3) {
                    Some(e) => e.1 += st.chars().count(),
                    None => w.push((*sz, st.chars().count())),
                }
            }
        }
        let size = w.iter().max_by_key(|(_, n)| *n).map(|(s, _)| *s).unwrap_or(0.0);
        out.push(Ln { t, size });
    }
    Ok(out)
}

fn plain(lines: &[Ln]) -> String {
    lines.iter().map(|l| l.t.as_str()).collect::<Vec<_>>().join("\n")
}

/// Body font size (character-weighted mode over all text pages) and the heading sizes above it.
fn heading_sizes(pages: &[Page]) -> Vec<f32> {
    let mut w: Vec<(f32, usize)> = vec![];
    for p in pages {
        if let Page::Text(ls) = p {
            for l in ls.iter().filter(|l| l.size > 0.0) {
                let n = l.t.chars().count();
                match w.iter_mut().find(|(s, _)| (s - l.size).abs() < 0.3) {
                    Some(e) => e.1 += n,
                    None => w.push((l.size, n)),
                }
            }
        }
    }
    let Some(body) = w.iter().max_by_key(|(_, n)| *n).map(|(s, _)| *s) else {
        return vec![];
    };
    let mut big: Vec<f32> = w.iter().map(|(s, _)| *s).filter(|s| *s >= body * 1.15).collect();
    big.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    big.truncate(3);
    big
}

fn with_headings(lines: &[Ln], sizes: &[f32]) -> String {
    lines
        .iter()
        .map(|l| {
            let rank = sizes.iter().position(|s| (s - l.size).abs() < 0.3);
            let short = l.t.chars().count() <= 160 && !l.t.ends_with(['.', ',', ';']);
            match rank {
                Some(r) if short && l.size > 0.0 => format!("{} {}", "#".repeat(r + 1), l.t),
                _ => l.t.clone(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn open_error(e: impl std::fmt::Display) -> AppError {
    let s = e.to_string();
    let l = s.to_lowercase();
    if l.contains("encrypt") || l.contains("password") {
        AppError::new(ErrorCode::DocEncrypted).with_detail(s)
    } else {
        failed("pdf", s)
    }
}

/// Share of characters that are replacement / private-use / control: a broken font map.
fn garbled(s: &str) -> bool {
    let mut total = 0usize;
    let mut bad = 0usize;
    for c in s.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        if c == '\u{FFFD}' || ('\u{E000}'..='\u{F8FF}').contains(&c) || c.is_control() {
            bad += 1;
        }
    }
    total >= 10 && bad * 5 > total
}

fn page_list(pages: &[u32]) -> String {
    let mut s = pages
        .iter()
        .take(20)
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if pages.len() > 20 {
        s.push_str(&format!(", ... ({} pages)", pages.len()));
    }
    s
}

pub fn convert(
    path: &Path,
    req: &IngestRequest,
    ocr: &dyn Ocr,
    lim: &Limits,
    progress: &mut Progress<'_>,
) -> Result<PdfOut> {
    let doc = catch_unwind(AssertUnwindSafe(|| PdfDocument::open(path)))
        .map_err(|_| failed("pdf", "parser panicked on this file"))?
        .map_err(open_error)?;
    if doc.is_encrypted() && !matches!(doc.authenticate(b""), Ok(true)) {
        return Err(AppError::new(ErrorCode::DocEncrypted).with_detail("the PDF needs a password"));
    }
    let n = doc.page_count().map_err(|e| failed("pdf", e.to_string()))? as u32;
    if n == 0 {
        return Err(AppError::new(ErrorCode::DocEmpty).with_detail("the PDF has no pages"));
    }
    let langs = languages_or_default(&req.languages);
    let ocr_on = req.ocr && ocr.available();

    let mut pages: Vec<Page> = Vec::with_capacity(n as usize);
    let mut ocr_pages = vec![];
    let mut skipped: Vec<(u32, String)> = vec![];
    let mut warnings = vec![];
    let mut ocr_attempted = 0u32;
    let mut ocr_failed = 0u32;
    let mut first_ocr_err: Option<AppError> = None;
    let mut unreadable = 0u32;

    for i in 0..n {
        progress.report(i, n)?;
        let pno = i + 1;
        let read = catch_unwind(AssertUnwindSafe(|| read_page(&doc, i as usize)))
            .unwrap_or_else(|_| Err("page parser panicked".into()));
        let (lines, read_err) = match read {
            Ok(l) => (l, None),
            Err(e) => (vec![], Some(e)),
        };
        let text = plain(&lines);
        if read_err.is_some() {
            unreadable += 1;
        }
        let weak = alnum_count(&text) < lim.scanned_min_chars || garbled(&text);
        if !weak {
            pages.push(Page::Text(lines));
            continue;
        }
        if ocr_on {
            ocr_attempted += 1;
            match ocr.recognize_pdf_page(path, i as usize, &langs) {
                Ok(o) if alnum_count(&o.text) >= alnum_count(&text) => {
                    if o.confidence.is_some_and(|c| c < 0.5) {
                        warnings.push(warn(
                            IngestWarningCode::OcrLowConfidence,
                            Some(pno),
                            o.confidence.map(|c| format!("{:.0}%", c * 100.0)),
                        ));
                    }
                    ocr_pages.push(pno);
                    pages.push(Page::Ocr(sanitize(&o.text)));
                    continue;
                }
                Ok(_) => {
                    // OCR found less than the text layer: keep the text layer
                    pages.push(Page::Text(lines));
                    continue;
                }
                Err(e) => {
                    ocr_failed += 1;
                    skipped.push((
                        pno,
                        format!(
                            "ocr failed: {}",
                            e.detail.clone().unwrap_or_else(|| e.code.to_string())
                        ),
                    ));
                    first_ocr_err.get_or_insert(e);
                }
            }
        } else if let Some(e) = read_err {
            skipped.push((pno, format!("unreadable: {e}")));
        } else if alnum_count(&text) == 0 {
            // a blank page is fine when it truly has no content; a scan without OCR is not
            let reason = if req.ocr {
                "no OCR engine available"
            } else {
                "OCR is switched off"
            };
            skipped.push((pno, format!("no text layer ({reason})")));
        } else {
            // little but real text and no OCR: keep it
            pages.push(Page::Text(lines));
            continue;
        }
        pages.push(Page::Missing(
            skipped.last().map(|s| s.1.clone()).unwrap_or_default(),
        ));
    }
    progress.report(n, n)?;

    let total_text: usize = pages
        .iter()
        .map(|p| match p {
            Page::Text(l) => l.iter().map(|x| alnum_count(&x.t)).sum(),
            Page::Ocr(t) => alnum_count(t),
            Page::Missing(_) => 0,
        })
        .sum();
    if total_text == 0 {
        if ocr_attempted > 0 && ocr_failed == ocr_attempted {
            return Err(first_ocr_err.unwrap_or_else(|| AppError::new(ErrorCode::OcrFailed)));
        }
        if unreadable == n {
            return Err(failed("pdf", "no page could be read"));
        }
        let reason = if !req.ocr {
            "scanned_ocr_off"
        } else if !ocr.available() {
            "scanned_no_ocr"
        } else {
            "blank"
        };
        return Err(AppError::new(ErrorCode::DocEmpty)
            .with_param("reason", reason)
            .with_detail("the PDF has no text layer and OCR produced nothing"));
    }

    // cleanup runs on text-layer pages only (OCR text has no running headers worth guessing at)
    let texts: Vec<Option<String>> = pages
        .iter()
        .map(|p| match p {
            Page::Text(l) => Some(plain(l)),
            _ => None,
        })
        .collect();
    let drop_keys = running_lines(&texts);
    let sizes = heading_sizes(&pages);
    let mut md = String::new();
    for (i, p) in pages.iter().enumerate() {
        md.push_str(&marker(i as u32 + 1));
        md.push('\n');
        match p {
            Page::Text(l) => {
                let kept = strip_edges(l, &drop_keys);
                md.push_str(&reflow(&with_headings(&kept, &sizes)))
            }
            Page::Ocr(t) => md.push_str(t.trim()),
            Page::Missing(why) => md.push_str(&format!("*[page {} could not be read: {why}]*", i + 1)),
        }
        md.push_str("\n\n");
    }
    if !ocr_pages.is_empty() {
        warnings.push(warn(
            IngestWarningCode::OcrUsed,
            None,
            Some(format!("pages {}", page_list(&ocr_pages))),
        ));
    }
    if !skipped.is_empty() {
        let list: Vec<u32> = skipped.iter().map(|s| s.0).collect();
        warnings.push(warn(
            IngestWarningCode::PagesSkipped,
            Some(list[0]),
            Some(format!("pages {}: {}", page_list(&list), skipped[0].1)),
        ));
    }
    Ok(PdfOut {
        md: tidy_pages(&md),
        pages: n,
        warnings,
        converter: if ocr_pages.is_empty() {
            "pdf-text"
        } else {
            "pdf-ocr"
        },
    })
}

fn tidy_pages(s: &str) -> String {
    tidy(s)
}

// ------------------------------------------------------------------------- cleanup

/// Comparison key of a line: markdown decoration out, case folded, digits collapsed.
fn key(line: &str) -> String {
    let mut k = String::new();
    let mut last_hash = false;
    for c in line.trim().trim_start_matches('#').chars() {
        if matches!(c, '*' | '_' | '`') {
            continue;
        }
        if c.is_ascii_digit() {
            if !last_hash {
                k.push('#');
            }
            last_hash = true;
        } else {
            last_hash = false;
            k.extend(c.to_lowercase());
        }
    }
    k.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_page_number(line: &str) -> bool {
    let k = key(line);
    let k = k.trim_matches(|c: char| c == '-' || c == '–' || c == '—' || c.is_whitespace());
    matches!(k, "#" | "#/#" | "# / #")
        || k.strip_prefix("page ").is_some_and(|r| r.starts_with('#'))
        || k.strip_prefix("стр. ").is_some_and(|r| r.starts_with('#'))
        || k.strip_prefix("стр ").is_some_and(|r| r.starts_with('#'))
        || k.strip_prefix("страница ").is_some_and(|r| r.starts_with('#'))
}

const EDGE: usize = 2;

fn edge_lines(text: &str) -> Vec<(usize, &str)> {
    let lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .collect();
    let mut out: Vec<(usize, &str)> = lines.iter().take(EDGE).copied().collect();
    for l in lines.iter().rev().take(EDGE) {
        if !out.iter().any(|(i, _)| *i == l.0) {
            out.push(*l);
        }
    }
    out
}

/// Keys of lines that repeat at the top/bottom of more than half of the text pages.
fn running_lines(pages: &[Option<String>]) -> Vec<String> {
    let texts: Vec<&String> = pages.iter().flatten().collect();
    if texts.len() < 3 {
        return vec![];
    }
    let big = |t: &str| t.lines().filter(|l| !l.trim().is_empty()).count() > 2 * EDGE;
    let big_pages = texts.iter().filter(|t| big(t)).count();
    let mut count: HashMap<String, usize> = HashMap::new();
    for t in texts.iter().filter(|t| big(t)) {
        let mut seen = std::collections::HashSet::new();
        for (_, l) in edge_lines(t) {
            let k = key(l);
            if !k.is_empty() && seen.insert(k.clone()) {
                *count.entry(k).or_default() += 1;
            }
        }
    }
    count
        .into_iter()
        .filter(|(_, c)| *c * 2 > big_pages && *c >= 2)
        .map(|(k, _)| k)
        .collect()
}

/// Removes the repeating header/footer lines and bare page numbers from the page edges.
fn strip_edges(lines: &[Ln], drop_keys: &[String]) -> Vec<Ln> {
    let text = plain(lines);
    let edges: Vec<usize> = edge_lines(&text).into_iter().map(|(i, _)| i).collect();
    lines
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            if !edges.contains(i) {
                return true;
            }
            !(drop_keys.contains(&key(&l.t)) || is_page_number(&l.t))
        })
        .map(|(_, l)| l.clone())
        .collect()
}

fn is_block_line(l: &str) -> bool {
    let t = l.trim_start();
    t.starts_with('#')
        || t.starts_with('|')
        || t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("> ")
        || t.starts_with("```")
        || t.starts_with("<!--")
        || t.starts_with("![")
        || t.starts_with("---")
        || t.split_once(". ")
            .is_some_and(|(a, _)| !a.is_empty() && a.len() < 4 && a.chars().all(|c| c.is_ascii_digit()))
}

/// Joins soft-wrapped lines of a paragraph and removes line-end hyphenation
/// ("поставщик-" + "ом" -> "поставщиком") when the next line continues in lower case.
fn reflow(text: &str) -> String {
    let mut out: Vec<String> = vec![];
    let mut para = String::new();
    let flush = |para: &mut String, out: &mut Vec<String>| {
        if !para.is_empty() {
            out.push(std::mem::take(para));
        }
    };
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            flush(&mut para, &mut out);
            out.push(String::new());
            continue;
        }
        if is_block_line(l) {
            flush(&mut para, &mut out);
            out.push(l.to_string());
            continue;
        }
        if para.is_empty() {
            para.push_str(l);
            continue;
        }
        let hyphen_join = para.ends_with('-')
            && para.chars().rev().nth(1).is_some_and(|c| c.is_alphabetic())
            && l.chars().next().is_some_and(|c| c.is_lowercase());
        if hyphen_join {
            para.pop();
            para.push_str(l);
        } else {
            para.push(' ');
            para.push_str(l);
        }
    }
    flush(&mut para, &mut out);
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyphenation_and_wrap() {
        let r = reflow("Поставщик обязуется пере-\nдать товар покупателю\nв срок.\n\nНовый абзац.");
        assert_eq!(
            r,
            "Поставщик обязуется передать товар покупателю в срок.\n\nНовый абзац."
        );
        // a real compound word at line end keeps its hyphen when the next line is capitalised
        assert_eq!(reflow("Санкт-\nПетербург"), "Санкт- Петербург");
    }

    #[test]
    fn page_numbers() {
        assert!(is_page_number("Page 12"));
        assert!(is_page_number("- 3 -"));
        assert!(is_page_number("Стр. 4"));
        assert!(is_page_number("3 / 10"));
        assert!(!is_page_number("Section 3 overview"));
    }

    #[test]
    fn repeating_headers_go() {
        let words = ["alpha", "bravo", "charlie", "delta"];
        let pages: Vec<Option<String>> = (1..=4)
            .map(|i| {
                Some(format!(
                    "ACME Corp - Confidential\n{w} first\n{w} second\n{w} third\n{w} fourth\nPage {i}",
                    w = words[i - 1]
                ))
            })
            .collect();
        let keys = running_lines(&pages);
        let lines: Vec<Ln> = pages[0]
            .as_ref()
            .unwrap()
            .lines()
            .map(|t| Ln {
                t: t.into(),
                size: 0.0,
            })
            .collect();
        let cleaned = plain(&strip_edges(&lines, &keys));
        assert_eq!(cleaned, "alpha first\nalpha second\nalpha third\nalpha fourth");
    }

    #[test]
    fn garbled_detection() {
        assert!(garbled(
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}ab\u{FFFD}"
        ));
        assert!(!garbled("обычный текст на русском языке"));
    }
}
