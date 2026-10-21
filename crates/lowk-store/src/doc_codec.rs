//! Document file codec (`type: doc`, architecture 4.3): frontmatter + converted Markdown body.
//!
//! ```markdown
//! ---
//! lowk: 1
//! type: doc
//! id: 01JC...
//! title: Договор поставки
//! kind: pdf
//! source: Договор поставки.pdf      # original, untouched, next to this file
//! original_name: spec_v3_final.pdf  # name as imported
//! source_sha256: 9f2c...
//! converter: pdf-text               # pdf-text | pdf-ocr | keynote-pdf | docx | pptx | xlsx | csv | txt | md | image-ocr
//! unit: page                        # what <!-- p:N --> counts: page | slide | sheet
//! pages: 12
//! status: ready
//! pinned: false
//! tokens: 4310
//! tags: [contract]
//! added: 2026-10-21T10:00:00Z
//! ---
//! <!-- p:1 -->
//! ...
//! ```
//! `error` and `warnings` are JSON strings (they carry UI params). Unknown keys survive a rewrite
//! byte for byte, so Obsidian properties added by the user are kept.
use crate::fm::{self, Frontmatter};
use lowk_types::{AppError, DocKind, IngestStatus, IngestWarning};
use serde::de::DeserializeOwned;
use serde::Serialize;

const ORDER: [&str; 18] = [
    "lowk",
    "type",
    "id",
    "title",
    "kind",
    "source",
    "original_name",
    "source_sha256",
    "converter",
    "unit",
    "pages",
    "status",
    "error",
    "warnings",
    "pinned",
    "tokens",
    "tags",
    "added",
];

#[derive(Clone, Debug, PartialEq)]
pub struct DocFm {
    pub id: String,
    pub title: String,
    pub kind: DocKind,
    /// Stored original, a file name next to the `.md`.
    pub source: String,
    pub original_name: String,
    pub source_sha256: String,
    pub converter: String,
    pub unit: String,
    pub pages: Option<u32>,
    pub status: IngestStatus,
    pub error: Option<AppError>,
    pub warnings: Vec<IngestWarning>,
    pub pinned: bool,
    pub tokens: u32,
    pub tags: Vec<String>,
    pub added: String,
    /// Everything else, in order, for byte-stable rewrites.
    pub raw: Frontmatter,
}

fn enum_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|j| j.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn from_enum<T: DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

/// Rough token estimate for budgets (no tokenizer dependency): Cyrillic costs about 2.3 characters
/// per token, other scripts about 4.
pub fn estimate_tokens(text: &str) -> u32 {
    let (mut cyr, mut other) = (0u64, 0u64);
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        if ('\u{0400}'..='\u{052F}').contains(&c) {
            cyr += 1;
        } else {
            other += 1;
        }
    }
    ((cyr as f64 / 2.3) + (other as f64 / 4.0)).ceil() as u32
}

/// Parses a document file. `None` when there is no frontmatter, no `id` or `type` is not `doc`.
pub fn parse(text: &str) -> Option<(DocFm, String)> {
    let norm = fm::normalize_eol(text);
    let (front, body) = fm::split(&norm);
    let f = front?;
    if f.get("type").map(fm::scalar).as_deref() != Some("doc") {
        return None;
    }
    let get = |k: &str| f.get(k).map(fm::scalar).unwrap_or_default();
    let id = get("id");
    if id.is_empty() {
        return None;
    }
    let kind = from_enum(&get("kind")).unwrap_or(DocKind::Txt);
    let status = from_enum(&get("status")).unwrap_or(IngestStatus::Queued);
    let doc = DocFm {
        id,
        title: get("title"),
        kind,
        source: get("source"),
        original_name: get("original_name"),
        source_sha256: get("source_sha256"),
        converter: get("converter"),
        unit: get("unit"),
        pages: get("pages").parse().ok(),
        status,
        error: serde_json::from_str(&get("error")).ok(),
        warnings: serde_json::from_str(&get("warnings")).unwrap_or_default(),
        pinned: get("pinned") == "true",
        tokens: get("tokens").parse().unwrap_or(0),
        tags: f.get("tags").map(fm::list).unwrap_or_default(),
        added: get("added"),
        raw: f,
    };
    Some((doc, body.to_string()))
}

pub fn render(d: &DocFm, body: &str) -> String {
    let mut out = d.raw.clone();
    let mut put = |k: &str, v: String| out.set_ordered(k, v, &ORDER);
    put("lowk", "1".into());
    put("type", "doc".into());
    put("id", d.id.clone());
    put("title", fm::quote(&d.title));
    put("kind", enum_str(&d.kind));
    put("source", fm::quote(&d.source));
    put("original_name", fm::quote(&d.original_name));
    put("source_sha256", d.source_sha256.clone());
    put("converter", d.converter.clone());
    if d.unit.is_empty() {
        out.remove("unit");
    } else {
        out.set_ordered("unit", d.unit.clone(), &ORDER);
    }
    match d.pages {
        Some(p) => out.set_ordered("pages", p.to_string(), &ORDER),
        None => out.remove("pages"),
    }
    out.set_ordered("status", enum_str(&d.status), &ORDER);
    match &d.error {
        Some(e) => out.set_ordered(
            "error",
            fm::quote(&serde_json::to_string(e).unwrap_or_default()),
            &ORDER,
        ),
        None => out.remove("error"),
    }
    if d.warnings.is_empty() {
        out.remove("warnings");
    } else {
        out.set_ordered(
            "warnings",
            fm::quote(&serde_json::to_string(&d.warnings).unwrap_or_default()),
            &ORDER,
        );
    }
    out.set_ordered("pinned", d.pinned.to_string(), &ORDER);
    out.set_ordered("tokens", d.tokens.to_string(), &ORDER);
    out.set_ordered("tags", fm::fmt_list(&d.tags), &ORDER);
    out.set_ordered("added", d.added.clone(), &ORDER);
    format!("{}{}", out.render(), body)
}

/// A fresh placeholder (`status: queued`) for an imported file.
#[allow(clippy::too_many_arguments)]
pub fn placeholder(
    id: &str,
    title: &str,
    kind: DocKind,
    source: &str,
    original_name: &str,
    sha: &str,
    pinned: bool,
    tags: Vec<String>,
    added: &str,
) -> DocFm {
    DocFm {
        id: id.into(),
        title: title.into(),
        kind,
        source: source.into(),
        original_name: original_name.into(),
        source_sha256: sha.into(),
        converter: String::new(),
        unit: String::new(),
        pages: None,
        status: IngestStatus::Queued,
        error: None,
        warnings: vec![],
        pinned,
        tokens: 0,
        tags,
        added: added.into(),
        raw: Frontmatter::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::{ErrorCode, IngestWarningCode};

    #[test]
    fn roundtrip_with_json_fields() {
        let mut d = placeholder(
            "01JC8Z3K6Q9X2M4N7R5T0V8WBD",
            "Договор: \"v3\"",
            DocKind::Pdf,
            "a.pdf",
            "a b.pdf",
            "abc",
            true,
            vec!["x, y".into()],
            "2026-10-21T10:00:00Z",
        );
        d.error = Some(
            AppError::new(ErrorCode::DocEmpty)
                .with_param("reason", "blank")
                .with_detail("no \"text\""),
        );
        d.warnings = vec![IngestWarning {
            code: IngestWarningCode::OcrUsed,
            page: Some(2),
            detail: Some("pages 1, 2".into()),
        }];
        d.pages = Some(3);
        d.unit = "page".into();
        let text = render(&d, "<!-- p:1 -->\nтекст\n");
        let (back, body) = parse(&text).unwrap();
        assert_eq!(body, "<!-- p:1 -->\nтекст\n");
        assert_eq!(
            back,
            DocFm {
                raw: back.raw.clone(),
                ..d.clone()
            }
        );
        assert_eq!(render(&back, &body), text, "rewrite is byte stable");
    }

    #[test]
    fn unknown_keys_survive() {
        let src = "---\nlowk: 1\ntype: doc\nid: 01JC8Z3K6Q9X2M4N7R5T0V8WBD\ntitle: T\nkind: txt\nsource: t.txt\nstatus: ready\nobsidian_prop: 42\n---\nbody\n";
        let (d, b) = parse(src).unwrap();
        assert!(render(&d, &b).contains("obsidian_prop: 42"));
    }

    #[test]
    fn not_a_doc() {
        assert!(parse("---\ntype: call\nid: x\n---\n").is_none());
        assert!(parse("no frontmatter").is_none());
    }

    #[test]
    fn tokens() {
        assert_eq!(estimate_tokens(""), 0);
        let ru = estimate_tokens(&"слово ".repeat(100));
        let en = estimate_tokens(&"words ".repeat(100));
        assert!(ru > en, "cyrillic costs more tokens: {ru} vs {en}");
    }
}
