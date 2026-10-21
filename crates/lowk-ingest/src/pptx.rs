//! PPTX to Markdown: one `<!-- p:N -->` unit per slide (in presentation order), title as `##`,
//! body paragraphs as bullets by level, tables to GFM, speaker notes. Pictures are not read
//! (`SlideImagesIgnored`).
use crate::md::{alnum_count, marker, sanitize, table, tidy, warn};
use crate::xml::{attr, walk, Archive, Ev};
use crate::{Limits, Progress};
use lowk_types::{AppError, ErrorCode, IngestWarning, IngestWarningCode, Result};
use std::collections::HashMap;
use std::path::Path;

pub struct Pptx {
    pub md: String,
    pub slides: u32,
    pub warnings: Vec<IngestWarning>,
}

fn rels(xml: &[u8]) -> Result<HashMap<String, (String, String)>> {
    let mut m = HashMap::new();
    walk(xml, "pptx", |e| {
        if let Ev::Start(n, a) = e {
            if n == "Relationship" {
                if let (Some(id), Some(t), Some(ty)) = (attr(&a, "Id"), attr(&a, "Target"), attr(&a, "Type"))
                {
                    m.insert(id.to_string(), (t.to_string(), ty.to_string()));
                }
            }
        }
        true
    })?;
    Ok(m)
}

/// Resolves `../notesSlides/x.xml` against `ppt/slides/`.
fn resolve(base_dir: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn slide_order(z: &mut Archive) -> Result<Vec<String>> {
    let mut order = vec![];
    if let (Some(pres), Some(r)) = (
        z.read("ppt/presentation.xml")?,
        z.read("ppt/_rels/presentation.xml.rels")?,
    ) {
        let rel = rels(&r)?;
        walk(&pres, "pptx", |e| {
            if let Ev::Start(n, a) = e {
                if n == "sldId" {
                    if let Some((t, _)) = attr(&a, "id").and_then(|id| rel.get(id)) {
                        order.push(resolve("ppt", t));
                    }
                }
            }
            true
        })?;
    }
    if order.is_empty() {
        // no usable presentation.xml: numeric order of slideN.xml
        let mut n: Vec<(u32, String)> = z
            .names()
            .into_iter()
            .filter_map(|f| {
                let num = f
                    .strip_prefix("ppt/slides/slide")?
                    .strip_suffix(".xml")?
                    .parse()
                    .ok()?;
                Some((num, f))
            })
            .collect();
        n.sort();
        order = n.into_iter().map(|(_, f)| f).collect();
    }
    Ok(order)
}

#[derive(Default)]
struct Slide {
    title: String,
    body: Vec<String>,
    tables: Vec<String>,
    has_pic: bool,
}

/// Text of one `<p:cSld>` tree: shapes in document order.
fn parse_slide(xml: &[u8], notes_mode: bool) -> Result<Slide> {
    let mut s = Slide::default();
    let mut in_sp = false;
    let mut ph: Option<String> = None;
    let mut para = String::new();
    let mut lvl = 0usize;
    let mut bullet_none = false;
    let mut in_para = false;
    let mut in_t = false;
    let mut fld = 0u32;
    let mut tbl = 0u32;
    let (mut rows, mut row, mut cell): (Vec<Vec<String>>, Vec<String>, String) =
        (vec![], vec![], String::new());
    walk(xml, "pptx", |e| {
        match e {
            Ev::Start(n, a) => match n.as_str() {
                "sp" => {
                    in_sp = true;
                    ph = None;
                }
                "ph" if in_sp => ph = Some(attr(&a, "type").unwrap_or("body").to_string()),
                "pic" => s.has_pic = true,
                "tbl" => {
                    tbl += 1;
                    rows.clear();
                }
                "tr" if tbl > 0 => row.clear(),
                "tc" if tbl > 0 => cell.clear(),
                "p" => {
                    in_para = true;
                    para.clear();
                    lvl = 0;
                    bullet_none = false;
                }
                "pPr" if in_para => lvl = attr(&a, "lvl").and_then(|v| v.parse().ok()).unwrap_or(0),
                "buNone" if in_para => bullet_none = true,
                "fld" => fld += 1,
                "br" if in_para => para.push(' '),
                "t" => in_t = true,
                _ => {}
            },
            Ev::Text(t) => {
                if in_t && in_para && fld == 0 {
                    para.push_str(&t);
                }
            }
            Ev::End(n) => match n.as_str() {
                "t" => in_t = false,
                "fld" => fld = fld.saturating_sub(1),
                "p" => {
                    in_para = false;
                    let text = sanitize(&para).split_whitespace().collect::<Vec<_>>().join(" ");
                    if tbl > 0 {
                        if !text.is_empty() {
                            if !cell.is_empty() {
                                cell.push(' ');
                            }
                            cell.push_str(&text);
                        }
                    } else if !text.is_empty() {
                        match ph.as_deref() {
                            Some("title" | "ctrTitle") => {
                                if !s.title.is_empty() {
                                    s.title.push(' ');
                                }
                                s.title.push_str(&text);
                            }
                            // slide number, date, footer, header and thumbnail placeholders
                            Some("sldNum" | "dt" | "ftr" | "hdr" | "sldImg") => {}
                            Some("subTitle") => s.body.push(text),
                            Some(_) if notes_mode => s.body.push(text),
                            _ => {
                                if bullet_none && lvl == 0 {
                                    s.body.push(text);
                                } else {
                                    s.body.push(format!("{}- {}", "  ".repeat(lvl.min(6)), text));
                                }
                            }
                        }
                    }
                }
                "tc" if tbl > 0 => row.push(std::mem::take(&mut cell)),
                "tr" if tbl > 0 => rows.push(std::mem::take(&mut row)),
                "tbl" => {
                    tbl = tbl.saturating_sub(1);
                    if tbl == 0 {
                        let t = table(&rows);
                        if !t.is_empty() {
                            s.tables.push(t.trim_end().to_string());
                        }
                    }
                }
                "sp" => {
                    in_sp = false;
                    ph = None;
                }
                _ => {}
            },
        }
        true
    })?;
    Ok(s)
}

pub fn convert(path: &Path, lim: &Limits, progress: &mut Progress<'_>) -> Result<Pptx> {
    let mut z = Archive::open(path, "pptx", lim.max_xml_bytes)?;
    let slides = slide_order(&mut z)?;
    if slides.is_empty() {
        return Err(AppError::new(ErrorCode::DocEmpty).with_detail("the presentation has no slides"));
    }
    let total = slides.len() as u32;
    let mut md = String::new();
    let mut warnings = vec![];
    let mut pic_slides = 0u32;
    let mut text_chars = 0usize;
    for (i, sp) in slides.iter().enumerate() {
        let n = i as u32 + 1;
        progress.report(i as u32, total)?;
        let xml = z
            .read(sp)?
            .ok_or_else(|| crate::md::failed("pptx", format!("{sp} is missing")))?;
        let s = parse_slide(&xml, false)?;
        // notes via the slide's own relationships
        let mut notes = String::new();
        let (dir, file) = sp.rsplit_once('/').unwrap_or(("", sp));
        if let Some(r) = z.read(&format!("{dir}/_rels/{file}.rels"))? {
            for (t, ty) in rels(&r)?.into_values() {
                if ty.ends_with("/notesSlide") {
                    if let Some(nx) = z.read(&resolve(dir, &t))? {
                        let ns = parse_slide(&nx, true)?;
                        notes = ns.body.join(" ");
                    }
                }
            }
        }
        md.push_str(&marker(n));
        md.push('\n');
        if !s.title.is_empty() {
            md.push_str(&format!("## {}\n\n", s.title));
        } else {
            md.push_str(&format!("## Slide {n}\n\n"));
        }
        if !s.body.is_empty() {
            md.push_str(&s.body.join("\n"));
            md.push_str("\n\n");
        }
        for t in &s.tables {
            md.push_str(t);
            md.push_str("\n\n");
        }
        if !notes.trim().is_empty() {
            md.push_str(&format!("> Notes: {}\n\n", notes.trim()));
        }
        text_chars += alnum_count(&s.title)
            + s.body.iter().map(|b| alnum_count(b)).sum::<usize>()
            + s.tables.iter().map(|b| alnum_count(b)).sum::<usize>()
            + alnum_count(&notes);
        if s.title.is_empty() && s.body.is_empty() && s.tables.is_empty() && notes.trim().is_empty() {
            md.push_str("*[no text on this slide]*\n\n");
        }
        if s.has_pic {
            pic_slides += 1;
        }
    }
    progress.report(total, total)?;
    if text_chars == 0 {
        return Err(AppError::new(ErrorCode::DocEmpty).with_detail("no slide has any text"));
    }
    if pic_slides > 0 {
        warnings.push(warn(
            IngestWarningCode::SlideImagesIgnored,
            None,
            Some(format!("{pic_slides} of {total} slides contain pictures")),
        ));
    }
    Ok(Pptx {
        md: tidy(&md),
        slides: total,
        warnings,
    })
}
