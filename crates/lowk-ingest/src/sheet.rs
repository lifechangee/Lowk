//! XLSX / CSV / TSV: one `<!-- p:N -->` unit per sheet, each sheet a Markdown table. Large sheets
//! are cut into row chunks with the header repeated and a `### name (rows a-b)` sub-heading so the
//! chunker never separates data from its column names. Row and column caps produce a
//! `SheetTruncated` warning, never a silent cut.
use crate::md::{failed, marker, sanitize, table, tidy, warn};
use crate::text;
use crate::{Limits, Progress};
use calamine::{Data, Reader};
use lowk_types::{AppError, ErrorCode, IngestWarning, IngestWarningCode, Result};
use std::path::Path;

pub struct Sheets {
    pub md: String,
    pub units: u32,
    pub warnings: Vec<IngestWarning>,
}

/// One sheet already read into text cells.
pub struct RawSheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    /// Source had more rows than we kept.
    pub rows_cut: Option<usize>,
    pub cols_cut: Option<usize>,
}

fn render(sheet: &RawSheet, unit: u32, lim: &Limits, md: &mut String, warnings: &mut Vec<IngestWarning>) {
    md.push_str(&marker(unit));
    md.push('\n');
    md.push_str(&format!("## {}\n\n", sanitize(&sheet.name).replace('\n', " ")));
    let header = &sheet.rows[0];
    let data = &sheet.rows[1..];
    if data.len() <= lim.chunk_rows {
        md.push_str(&table(&sheet.rows));
        md.push('\n');
    } else {
        for (i, chunk) in data.chunks(lim.chunk_rows).enumerate() {
            let from = i * lim.chunk_rows + 1;
            let to = from + chunk.len() - 1;
            md.push_str(&format!(
                "### {} (rows {from}-{to})\n\n",
                sanitize(&sheet.name).replace('\n', " ")
            ));
            let mut rows = Vec::with_capacity(chunk.len() + 1);
            rows.push(header.clone());
            rows.extend(chunk.iter().cloned());
            md.push_str(&table(&rows));
            md.push('\n');
        }
    }
    if sheet.rows_cut.is_some() || sheet.cols_cut.is_some() {
        let mut d = Vec::new();
        if let Some(r) = sheet.rows_cut {
            if r == usize::MAX {
                d.push(format!("rows: kept the first {}, more exist", lim.max_rows));
            } else {
                d.push(format!("rows: kept {} of {}", lim.max_rows, r));
            }
        }
        if let Some(c) = sheet.cols_cut {
            d.push(format!("columns: kept {} of {}", lim.max_cols, c));
        }
        warnings.push(warn(
            IngestWarningCode::SheetTruncated,
            Some(unit),
            Some(d.join("; ")),
        ));
        md.push_str("*[sheet truncated: ");
        md.push_str(&d.join("; "));
        md.push_str("]*\n\n");
    }
}

fn finish(sheets: Vec<RawSheet>, lim: &Limits, fmt: &str) -> Result<Sheets> {
    let mut md = String::new();
    let mut warnings = vec![];
    let mut unit = 0;
    for s in sheets.iter().filter(|s| has_content(&s.rows)) {
        unit += 1;
        render(s, unit, lim, &mut md, &mut warnings);
    }
    if unit == 0 {
        return Err(AppError::new(ErrorCode::DocEmpty).with_detail(format!("{fmt}: no cell has any content")));
    }
    Ok(Sheets {
        md: tidy(&md),
        units: unit,
        warnings,
    })
}

fn has_content(rows: &[Vec<String>]) -> bool {
    rows.iter().any(|r| r.iter().any(|c| !c.trim().is_empty()))
}

// ---------------------------------------------------------------------------------------- CSV

fn detect_delimiter(text: &str, hint_tab: bool) -> char {
    if hint_tab {
        return '\t';
    }
    let sample: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(20).collect();
    let mut best = (',', 0usize);
    for d in [',', ';', '\t', '|'] {
        let counts: Vec<usize> = sample.iter().map(|l| l.matches(d).count()).collect();
        let first = counts.first().copied().unwrap_or(0);
        if first == 0 {
            continue;
        }
        let consistent = counts.iter().filter(|&&c| c == first).count();
        let score = consistent * 100 + first;
        if score > best.1 {
            best = (d, score);
        }
    }
    best.0
}

/// RFC 4180-ish: quotes, doubled quotes, newlines inside quotes. Stops after `max_rows + 1` rows
/// (+1 so truncation can be reported with a total count of at least that many).
fn parse_csv(text: &str, delim: char, max_rows: usize, max_cols: usize) -> (Vec<Vec<String>>, bool, usize) {
    let mut rows: Vec<Vec<String>> = vec![];
    let mut row: Vec<String> = vec![];
    let mut cell = String::new();
    let mut in_q = false;
    let mut cut = false;
    let mut widest = 0;
    let mut chars = text.chars().peekable();
    let mut push_row = |row: &mut Vec<String>, rows: &mut Vec<Vec<String>>| {
        widest = widest.max(row.len());
        row.truncate(max_cols);
        rows.push(std::mem::take(row));
    };
    while let Some(c) = chars.next() {
        if in_q {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    in_q = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => in_q = true,
            c if c == delim => row.push(std::mem::take(&mut cell)),
            '\n' => {
                row.push(std::mem::take(&mut cell));
                push_row(&mut row, &mut rows);
                if rows.len() > max_rows {
                    cut = true;
                    break;
                }
            }
            '\r' => {}
            c => cell.push(c),
        }
    }
    if !cut && (!cell.is_empty() || !row.is_empty()) {
        row.push(cell);
        push_row(&mut row, &mut rows);
    }
    (rows, cut, widest)
}

pub fn convert_csv(path: &Path, lim: &Limits, progress: &mut Progress<'_>) -> Result<Sheets> {
    let bytes = std::fs::read(path)?;
    let (raw, _) = text::decode(&bytes)?;
    drop(bytes);
    let hint_tab = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("tsv"));
    let delim = detect_delimiter(&raw, hint_tab);
    let (mut rows, cut, widest) = parse_csv(&raw, delim, lim.max_rows + 1, lim.max_cols);
    let rows_cut = if rows.len() > lim.max_rows {
        rows.truncate(lim.max_rows);
        Some(usize::MAX)
    } else if cut {
        Some(usize::MAX)
    } else {
        None
    };
    let cols_cut = (widest > lim.max_cols).then_some(widest);
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Sheet1".into());
    progress.report(1, 1)?;
    finish(
        vec![RawSheet {
            name,
            rows,
            rows_cut,
            cols_cut,
        }],
        lim,
        "csv",
    )
}

// ---------------------------------------------------------------------------------------- XLSX

fn fmt_float(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 {
        return format!("{}", f as i64);
    }
    // Excel "General" shows 15 significant digits.
    let rounded: f64 = format!("{f:.14e}").parse().unwrap_or(f);
    rounded.to_string()
}

fn fmt_cell(d: &Data) -> String {
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => fmt_float(*f),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Data::DateTime(dt) => {
            if dt.is_datetime() {
                let (y, mo, da, h, mi, s, _) = dt.to_ymd_hms_milli();
                if h == 0 && mi == 0 && s == 0 {
                    format!("{y:04}-{mo:02}-{da:02}")
                } else if s == 0 {
                    format!("{y:04}-{mo:02}-{da:02} {h:02}:{mi:02}")
                } else {
                    format!("{y:04}-{mo:02}-{da:02} {h:02}:{mi:02}:{s:02}")
                }
            } else {
                // duration in days
                fmt_float(dt.as_f64())
            }
        }
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#{e:?}"),
    }
}

pub fn convert_xlsx(path: &Path, lim: &Limits, progress: &mut Progress<'_>) -> Result<Sheets> {
    // Legacy / password protected Office files are OLE containers, not zip.
    let mut magic = [0u8; 8];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(path)?;
        let n = f.read(&mut magic)?;
        if n >= 8 && magic == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
            return Err(AppError::new(ErrorCode::DocEncrypted)
                .with_detail("OLE container: password protected or legacy workbook"));
        }
    }
    let mut wb = calamine::open_workbook_auto(path).map_err(|e| match e {
        calamine::Error::Xlsx(calamine::XlsxError::Password) => AppError::new(ErrorCode::DocEncrypted),
        e => failed("xlsx", e.to_string()),
    })?;
    let metas: Vec<(String, bool)> = wb
        .sheets_metadata()
        .iter()
        .map(|s| {
            (
                s.name.clone(),
                matches!(s.visible, calamine::SheetVisible::Visible),
            )
        })
        .collect();
    let total = metas.len() as u32;
    let mut sheets = vec![];
    for (i, (name, visible)) in metas.iter().enumerate() {
        progress.report(i as u32, total)?;
        if !visible {
            continue;
        }
        let range = wb
            .worksheet_range(name)
            .map_err(|e| failed("xlsx", format!("sheet '{name}': {e}")))?;
        let (h, w) = range.get_size();
        let cols = w.min(lim.max_cols);
        let mut rows = Vec::with_capacity(h.min(lim.max_rows));
        for r in range.rows().take(lim.max_rows) {
            rows.push(r.iter().take(cols).map(fmt_cell).collect::<Vec<_>>());
        }
        sheets.push(RawSheet {
            name: name.clone(),
            rows,
            rows_cut: (h > lim.max_rows).then_some(h),
            cols_cut: (w > lim.max_cols).then_some(w),
        });
    }
    progress.report(total, total)?;
    finish(sheets, lim, "xlsx")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_and_delims() {
        let t = "a;b\n1;\"x;y\"\n\"q\"\"r\";3\n";
        let d = detect_delimiter(t, false);
        assert_eq!(d, ';');
        let (rows, cut, _) = parse_csv(t, d, 100, 10);
        assert!(!cut);
        assert_eq!(rows[1], vec!["1", "x;y"]);
        assert_eq!(rows[2], vec!["q\"r", "3"]);
    }

    #[test]
    fn floats_like_excel() {
        assert_eq!(fmt_float(3.0), "3");
        assert_eq!(fmt_float(120.5), "120.5");
        assert_eq!(fmt_float(0.1 + 0.2), "0.3");
    }
}
