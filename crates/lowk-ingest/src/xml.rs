//! Tiny owned-event XML walker over `quick-xml` (local names only, entities resolved) and a
//! bounded zip reader. Malformed XML is an error, never a panic.
use crate::md::failed;
use lowk_types::{AppError, ErrorCode, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Read;

#[derive(Debug)]
pub enum Ev {
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
}

pub fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn name_of(b: &str) -> String {
    b.to_string()
}

fn attrs_of(e: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    e.attributes()
        .flatten()
        .map(|a| {
            (
                name_of(a.key.local_name().as_ref()),
                a.normalized_value(quick_xml::XmlVersion::default())
                    .map(|v| v.into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect()
}

/// Walks `xml`; `f` returns `false` to stop early.
pub fn walk(xml: &[u8], fmt: &str, mut f: impl FnMut(Ev) -> bool) -> Result<()> {
    let mut r = Reader::from_reader(xml);
    r.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let ev = r
            .read_event_into(&mut buf)
            .map_err(|e| failed(fmt, format!("xml error at byte {}: {e}", r.buffer_position())))?;
        let go = match ev {
            Event::Start(e) => f(Ev::Start(name_of(e.local_name().as_ref()), attrs_of(&e))),
            Event::Empty(e) => {
                let n = name_of(e.local_name().as_ref());
                f(Ev::Start(n.clone(), attrs_of(&e))) && f(Ev::End(n))
            }
            Event::End(e) => f(Ev::End(name_of(e.local_name().as_ref()))),
            Event::Text(t) => f(Ev::Text(t.xml10_content().into_owned())),
            Event::CData(t) => f(Ev::Text(t.xml10_content().into_owned())),
            Event::GeneralRef(g) => {
                let s = if let Ok(Some(c)) = g.resolve_char_ref() {
                    c.to_string()
                } else {
                    match &*g.into_inner() {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        _ => String::new(),
                    }
                };
                f(Ev::Text(s))
            }
            Event::Eof => break,
            _ => true,
        };
        if !go {
            break;
        }
    }
    Ok(())
}

/// A zip archive with size guards (zip bombs) and typed errors.
pub struct Archive {
    z: zip::ZipArchive<std::fs::File>,
    fmt: &'static str,
    max_entry: u64,
}

impl Archive {
    pub fn open(path: &std::path::Path, fmt: &'static str, max_entry: u64) -> Result<Self> {
        let mut f = std::fs::File::open(path)?;
        let mut magic = [0u8; 8];
        let n = f.read(&mut magic)?;
        if n >= 8 && magic == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
            // Password protected OOXML files and legacy binary Office files are OLE containers.
            return Err(AppError::new(ErrorCode::DocEncrypted)
                .with_param("format", fmt)
                .with_detail("OLE container: password protected or legacy binary Office file"));
        }
        let f = std::fs::File::open(path)?;
        let z = zip::ZipArchive::new(f).map_err(|e| failed(fmt, format!("not a valid {fmt} file: {e}")))?;
        Ok(Self { z, fmt, max_entry })
    }

    pub fn names(&self) -> Vec<String> {
        self.z
            .file_names()
            .filter_map(|n| n.ok().map(|c| c.into_owned()))
            .collect()
    }

    /// `None` when the entry does not exist.
    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        let fmt = self.fmt;
        let max = self.max_entry;
        let entry = match self.z.by_name(name) {
            Ok(e) => e,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(failed(fmt, format!("{name}: {e}"))),
        };
        if entry.size() > max {
            return Err(AppError::new(ErrorCode::DocTooLarge)
                .with_param("format", fmt)
                .with_detail(format!("{name} is {} bytes uncompressed", entry.size())));
        }
        let mut out = Vec::with_capacity(entry.size() as usize);
        entry
            .take(max)
            .read_to_end(&mut out)
            .map_err(|e| failed(fmt, format!("{name}: {e}")))?;
        Ok(Some(out))
    }
}
