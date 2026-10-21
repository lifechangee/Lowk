//! DOCX to Markdown: headings (style names in any UI language, outline levels), lists (bullet or
//! numbered, nested), tables to GFM, footnotes/endnotes, page markers where the file records page
//! breaks. Pictures and charts are not read.
use crate::md::{alnum_count, marker, sanitize, table, tidy, warn};
use crate::xml::{attr, walk, Archive, Ev};
use crate::Limits;
use lowk_types::{AppError, ErrorCode, IngestWarning, IngestWarningCode, Result};
use std::collections::HashMap;
use std::path::Path;

pub struct Docx {
    pub md: String,
    pub pages: Option<u32>,
    pub warnings: Vec<IngestWarning>,
}

#[derive(Default)]
struct Styles {
    /// styleId -> heading level
    heading: HashMap<String, u8>,
}

fn heading_level_from_name(name: &str) -> Option<u8> {
    let n = name.trim().to_lowercase();
    if n == "title" || n == "заголовок" || n == "название" {
        return Some(1);
    }
    for p in ["heading ", "heading", "заголовок ", "заголовок"] {
        if let Some(rest) = n.strip_prefix(p) {
            if let Ok(l) = rest.trim().parse::<u8>() {
                return Some(l.clamp(1, 6));
            }
        }
    }
    None
}

fn parse_styles(xml: &[u8]) -> Result<Styles> {
    let mut s = Styles::default();
    let mut cur: Option<String> = None;
    walk(xml, "docx", |e| {
        match e {
            Ev::Start(n, a) if n == "style" => cur = attr(&a, "styleId").map(str::to_string),
            Ev::Start(n, a) if n == "name" => {
                if let (Some(id), Some(v)) = (&cur, attr(&a, "val")) {
                    if let Some(l) = heading_level_from_name(v) {
                        s.heading.insert(id.clone(), l);
                    }
                }
            }
            Ev::Start(n, a) if n == "outlineLvl" => {
                if let (Some(id), Some(v)) = (&cur, attr(&a, "val").and_then(|v| v.parse::<u8>().ok())) {
                    if v < 9 {
                        s.heading.entry(id.clone()).or_insert((v + 1).min(6));
                    }
                }
            }
            Ev::End(n) if n == "style" => cur = None,
            _ => {}
        }
        true
    })?;
    Ok(s)
}

/// (numId, ilvl) -> ordered?
fn parse_numbering(xml: &[u8]) -> Result<HashMap<(String, u32), bool>> {
    let mut abs_lvl: HashMap<(String, u32), bool> = HashMap::new();
    let mut num_abs: HashMap<String, String> = HashMap::new();
    let (mut cur_abs, mut cur_lvl, mut cur_num): (Option<String>, u32, Option<String>) = (None, 0, None);
    walk(xml, "docx", |e| {
        match e {
            Ev::Start(n, a) => match n.as_str() {
                "abstractNum" => cur_abs = attr(&a, "abstractNumId").map(str::to_string),
                "lvl" => cur_lvl = attr(&a, "ilvl").and_then(|v| v.parse().ok()).unwrap_or(0),
                "numFmt" => {
                    if let (Some(id), Some(v)) = (&cur_abs, attr(&a, "val")) {
                        abs_lvl.insert((id.clone(), cur_lvl), v != "bullet" && v != "none");
                    }
                }
                "num" => cur_num = attr(&a, "numId").map(str::to_string),
                "abstractNumId" if cur_num.is_some() => {
                    if let (Some(n), Some(v)) = (&cur_num, attr(&a, "val")) {
                        num_abs.insert(n.clone(), v.to_string());
                    }
                }
                _ => {}
            },
            Ev::End(n) if n == "abstractNum" => cur_abs = None,
            Ev::End(n) if n == "num" => cur_num = None,
            _ => {}
        }
        true
    })?;
    let mut out = HashMap::new();
    for (num, abs) in num_abs {
        for ((a, lvl), ord) in &abs_lvl {
            if *a == abs {
                out.insert((num.clone(), *lvl), *ord);
            }
        }
    }
    Ok(out)
}

/// id -> text of footnotes / endnotes.
fn parse_notes(xml: &[u8], prefix: &str) -> Result<Vec<(String, String)>> {
    let mut out = vec![];
    let (mut id, mut text, mut skip): (Option<String>, String, bool) = (None, String::new(), false);
    let (item, _) = if prefix == "e" {
        ("endnote", ())
    } else {
        ("footnote", ())
    };
    walk(xml, "docx", |e| {
        match e {
            Ev::Start(n, a) if n == item => {
                skip = matches!(
                    attr(&a, "type"),
                    Some("separator" | "continuationSeparator" | "continuationNotice")
                );
                id = attr(&a, "id").map(str::to_string);
                text.clear();
            }
            Ev::Text(t) if id.is_some() && !skip => text.push_str(&t),
            Ev::End(n) if n == "p" && id.is_some() && !skip => text.push(' '),
            Ev::End(n) if n == item => {
                if let Some(i) = id.take() {
                    if !skip {
                        let t = sanitize(&text);
                        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                        if !t.is_empty() {
                            out.push((format!("{prefix}{i}"), t));
                        }
                    }
                }
            }
            _ => {}
        }
        true
    })?;
    Ok(out)
}

pub fn convert(path: &Path, lim: &Limits) -> Result<Docx> {
    let mut z = Archive::open(path, "docx", lim.max_xml_bytes)?;
    let doc = z
        .read("word/document.xml")?
        .ok_or_else(|| crate::md::failed("docx", "word/document.xml is missing"))?;
    let styles = match z.read("word/styles.xml")? {
        Some(x) => parse_styles(&x)?,
        None => Styles::default(),
    };
    let numbering = match z.read("word/numbering.xml")? {
        Some(x) => parse_numbering(&x)?,
        None => HashMap::new(),
    };
    let mut notes: HashMap<String, String> = HashMap::new();
    if let Some(x) = z.read("word/footnotes.xml")? {
        notes.extend(parse_notes(&x, "")?);
    }
    if let Some(x) = z.read("word/endnotes.xml")? {
        notes.extend(parse_notes(&x, "e")?);
    }

    let hay = String::from_utf8_lossy(&doc);
    let explicit_breaks = hay.contains("w:type=\"page\"") || hay.contains("<w:pageBreakBefore");
    drop(hay);

    struct St {
        blocks: Vec<String>,
        page: u32,
        used_notes: Vec<String>,
        warnings: Vec<IngestWarning>,
        merged: bool,
        // paragraph
        in_p: bool,
        in_ppr: bool,
        style: Option<String>,
        outline: Option<u8>,
        num: Option<(String, u32)>,
        text: String,
        break_after: bool,
        break_before: bool,
        // table
        tbl_depth: u32,
        rows: Vec<Vec<String>>,
        row: Vec<String>,
        cell: String,
        skip: u32,
        in_t: bool,
        last_list: bool,
    }
    let mut st = St {
        blocks: vec![],
        page: 1,
        used_notes: vec![],
        warnings: vec![],
        merged: false,
        in_p: false,
        in_ppr: false,
        style: None,
        outline: None,
        num: None,
        text: String::new(),
        break_after: false,
        break_before: false,
        tbl_depth: 0,
        rows: vec![],
        row: vec![],
        cell: String::new(),
        skip: 0,
        in_t: false,
        last_list: false,
    };

    walk(&doc, "docx", |e| {
        match e {
            Ev::Start(n, a) => {
                if st.skip > 0 {
                    st.skip += 1;
                    return true;
                }
                match n.as_str() {
                    // duplicated text boxes: keep the modern branch only
                    "Fallback" => st.skip = 1,
                    "tbl" => {
                        st.tbl_depth += 1;
                        if st.tbl_depth == 1 {
                            st.rows.clear();
                        }
                    }
                    "tr" if st.tbl_depth == 1 => st.row.clear(),
                    "tc" if st.tbl_depth == 1 => st.cell.clear(),
                    "gridSpan" | "vMerge" => st.merged = true,
                    "p" => {
                        st.in_p = true;
                        st.style = None;
                        st.outline = None;
                        st.num = None;
                        st.text.clear();
                        st.break_after = false;
                        st.break_before = false;
                    }
                    "pPr" if st.in_p => st.in_ppr = true,
                    "pStyle" if st.in_ppr => st.style = attr(&a, "val").map(str::to_string),
                    "outlineLvl" if st.in_ppr => st.outline = attr(&a, "val").and_then(|v| v.parse().ok()),
                    "ilvl" if st.in_ppr => {
                        let l = attr(&a, "val").and_then(|v| v.parse().ok()).unwrap_or(0);
                        let id = st.num.as_ref().map(|x| x.0.clone()).unwrap_or_default();
                        st.num = Some((id, l));
                    }
                    "numId" if st.in_ppr => {
                        let l = st.num.as_ref().map(|x| x.1).unwrap_or(0);
                        st.num = Some((attr(&a, "val").unwrap_or("0").to_string(), l));
                    }
                    "pageBreakBefore" if st.in_ppr => st.break_before = true,
                    "lastRenderedPageBreak" if !explicit_breaks && st.in_p => st.break_before = true,
                    "br" if st.in_p && explicit_breaks && attr(&a, "type") == Some("page") => {
                        st.break_after = true
                    }
                    "tab" if st.in_p && !st.in_ppr => st.text.push(' '),
                    "br" if st.in_p => st.text.push('\n'),
                    "footnoteReference" | "endnoteReference" if st.in_p => {
                        if let Some(id) = attr(&a, "id") {
                            let key = if n == "endnoteReference" {
                                format!("e{id}")
                            } else {
                                id.to_string()
                            };
                            if notes.contains_key(&key) {
                                st.text.push_str(&format!("[^{key}]"));
                                st.used_notes.push(key);
                            }
                        }
                    }
                    "t" => st.in_t = true,
                    _ => {}
                }
            }
            Ev::Text(t) => {
                if st.skip == 0 && st.in_t && st.in_p {
                    st.text.push_str(&t);
                }
            }
            Ev::End(n) => {
                if st.skip > 0 {
                    st.skip -= 1;
                    return true;
                }
                match n.as_str() {
                    "t" => st.in_t = false,
                    "pPr" => st.in_ppr = false,
                    "p" => {
                        st.in_p = false;
                        let raw = sanitize(&st.text);
                        let text = raw
                            .split('\n')
                            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
                            .collect::<Vec<_>>()
                            .join("  \n");
                        let text = text.trim().to_string();
                        if st.break_before && !(st.blocks.is_empty() && st.page == 1) {
                            st.page += 1;
                            st.blocks.push(marker(st.page));
                            st.last_list = false;
                        }
                        if st.tbl_depth > 0 {
                            if !text.is_empty() {
                                if !st.cell.is_empty() {
                                    st.cell.push(' ');
                                }
                                st.cell.push_str(&text.replace("  \n", " "));
                            }
                        } else if !text.is_empty() {
                            let level = st
                                .style
                                .as_ref()
                                .and_then(|s| {
                                    styles.heading.get(s).copied().or_else(|| {
                                        // styles.xml may be absent: fall back to the style id itself
                                        heading_level_from_name(
                                            &s.replace("Heading", "heading ").replace("Title", "title"),
                                        )
                                    })
                                })
                                .or(st.outline.map(|o| (o + 1).min(6)));
                            if let Some(l) = level {
                                st.blocks.push(format!(
                                    "{} {}",
                                    "#".repeat(l as usize),
                                    text.replace("  \n", " ")
                                ));
                                st.last_list = false;
                            } else if let Some((num, lvl)) = st.num.clone().filter(|(n, _)| n != "0") {
                                let ordered = numbering.get(&(num, lvl)).copied().unwrap_or(false);
                                let bullet = if ordered { "1." } else { "-" };
                                let item = format!(
                                    "{}{} {}",
                                    "  ".repeat(lvl.min(6) as usize),
                                    bullet,
                                    text.replace("  \n", " ")
                                );
                                if st.last_list {
                                    // keep list items adjacent
                                    if let Some(last) = st.blocks.last_mut() {
                                        last.push('\n');
                                        last.push_str(&item);
                                    }
                                } else {
                                    st.blocks.push(item);
                                }
                                st.last_list = true;
                            } else {
                                st.blocks.push(text);
                                st.last_list = false;
                            }
                        }
                        if st.break_after {
                            st.page += 1;
                            st.blocks.push(marker(st.page));
                            st.last_list = false;
                        }
                    }
                    "tc" if st.tbl_depth == 1 => {
                        let c = std::mem::take(&mut st.cell);
                        st.row.push(c);
                    }
                    "tr" if st.tbl_depth == 1 => {
                        let r = std::mem::take(&mut st.row);
                        st.rows.push(r);
                    }
                    "tbl" => {
                        st.tbl_depth = st.tbl_depth.saturating_sub(1);
                        if st.tbl_depth == 0 {
                            let t = table(&st.rows);
                            if !t.is_empty() {
                                st.blocks.push(t.trim_end().to_string());
                            }
                            st.last_list = false;
                        }
                    }
                    _ => {}
                }
            }
        }
        true
    })?;

    if st.merged {
        st.warnings.push(warn(
            IngestWarningCode::TablesFlattened,
            None,
            Some("merged cells were flattened".into()),
        ));
    }
    let mut body = st.blocks.join("\n\n");
    if !st.used_notes.is_empty() {
        let mut seen = std::collections::HashSet::new();
        body.push_str("\n\n## Notes\n\n");
        for k in st.used_notes.iter().filter(|k| seen.insert((*k).clone())) {
            body.push_str(&format!("[^{k}]: {}\n", notes[k]));
        }
    }
    if alnum_count(&body) == 0 {
        return Err(AppError::new(ErrorCode::DocEmpty).with_detail("the document has no text"));
    }
    let pages = (st.page > 1).then_some(st.page);
    let md = if pages.is_some() {
        format!("{}\n\n{}", marker(1), body)
    } else {
        body
    };
    Ok(Docx {
        md: tidy(&md),
        pages,
        warnings: st.warnings,
    })
}
