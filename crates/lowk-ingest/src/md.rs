//! Markdown building blocks shared by the converters.
use lowk_types::{AppError, ErrorCode, IngestWarning, IngestWarningCode};

/// `<!-- p:N -->` page / slide / sheet marker (1-based).
pub fn marker(n: u32) -> String {
    format!("<!-- p:{n} -->")
}

/// Typed error helper: `IngestFailed` with a format param and an English detail.
pub fn failed(format: &str, detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::IngestFailed)
        .with_param("format", format)
        .with_detail(detail)
}

pub fn warn(code: IngestWarningCode, page: Option<u32>, detail: Option<String>) -> IngestWarning {
    IngestWarning { code, page, detail }
}

/// Text that came from a document and goes into Markdown that the app parses (page markers) and
/// that is later sent to a model: control characters out, `<!--` neutralised so document content
/// can never forge a page marker or smuggle HTML comments. Unicode is left alone.
pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{0}' | '\u{feff}' | '\u{200b}' => {}
            '\r' => {}
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    if out.contains("<!--") {
        out = out.replace("<!--", "&lt;!--");
    }
    out
}

fn cell_text(s: &str) -> String {
    let s = sanitize(s);
    let s = s.replace(['\n', '\t'], " ").replace('|', "\\|");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Rows to a GFM table. The first row is the header. Rows are padded to the widest row; fully
/// empty trailing columns and rows are dropped. Very wide tables (more than `WIDE` columns) become
/// "column: value" lists, which both read and chunk better than an unreadable pipe table.
pub fn table(rows: &[Vec<String>]) -> String {
    const WIDE: usize = 14;
    let mut rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|c| cell_text(c)).collect())
        .collect();
    while rows.last().is_some_and(|r| r.iter().all(|c| c.is_empty())) {
        rows.pop();
    }
    let mut width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    while width > 0 && rows.iter().all(|r| r.get(width - 1).is_none_or(|c| c.is_empty())) {
        width -= 1;
    }
    if rows.is_empty() || width == 0 {
        return String::new();
    }
    for r in &mut rows {
        r.resize(width, String::new());
    }
    let mut out = String::new();
    if width > WIDE {
        let head = rows[0].clone();
        for r in &rows[1..] {
            let parts: Vec<String> = r
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.is_empty())
                .map(|(i, c)| {
                    let h = if head[i].is_empty() {
                        format!("col{}", i + 1)
                    } else {
                        head[i].clone()
                    };
                    format!("{h}: {c}")
                })
                .collect();
            if !parts.is_empty() {
                out.push_str("- ");
                out.push_str(&parts.join("; "));
                out.push('\n');
            }
        }
        return out;
    }
    let line = |r: &[String]| format!("| {} |\n", r.join(" | "));
    out.push_str(&line(&rows[0]));
    out.push_str(&format!("|{}|\n", vec![" --- "; width].join("|")));
    for r in &rows[1..] {
        out.push_str(&line(r));
    }
    out
}

/// Number of letters and digits; the "is there real text" measure.
pub fn alnum_count(s: &str) -> usize {
    s.chars().filter(|c| c.is_alphanumeric()).count()
}

/// Collapses runs of 3+ newlines to a blank line, trims, ends with a single newline.
pub fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank = 0;
    for line in s.lines() {
        let l = line.trim_end();
        if l.is_empty() {
            blank += 1;
            if blank <= 1 && !out.is_empty() {
                out.push('\n');
            }
        } else {
            blank = 0;
            out.push_str(l);
            out.push('\n');
        }
    }
    let t = out.trim_end();
    format!("{t}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_neutralised() {
        assert_eq!(sanitize("a <!-- p:9 --> b\u{0}"), "a &lt;!-- p:9 --> b");
    }

    #[test]
    fn table_basic_and_escape() {
        let t = table(&[
            vec!["A".into(), "B|C".into()],
            vec!["1".into(), "x\ny".into()],
            vec![String::new(), String::new()],
        ]);
        assert_eq!(t, "| A | B\\|C |\n| --- | --- |\n| 1 | x y |\n");
    }

    #[test]
    fn wide_table_becomes_list() {
        let head: Vec<String> = (0..20).map(|i| format!("h{i}")).collect();
        let row: Vec<String> = (0..20).map(|i| format!("v{i}")).collect();
        let t = table(&[head, row]);
        assert!(t.starts_with("- h0: v0; h1: v1"));
    }
}
