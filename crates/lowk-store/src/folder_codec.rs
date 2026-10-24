//! `_folder.md` codec (architecture section 4.3): frontmatter (id, title, tags,
//! cloud policy, default template), `## Instructions`, `## Documents` list.
use crate::fm::{self, Frontmatter};
use lowk_types::{MicPreset, RetxPreset};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FPart {
    Instructions(String, String),
    /// Heading as written, wikilink targets.
    Documents(String, Vec<String>),
    Other(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderDoc {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub allow_cloud: bool,
    pub default_template: Option<String>,
    /// `record_mic: on|off` in the frontmatter; absent = inherit the global setting.
    pub mic: MicPreset,
    /// `retranscribe: on|off` in the frontmatter; absent = inherit `stt.retranscribe_after`.
    pub retx: RetxPreset,
    pub raw: Frontmatter,
    /// Free text before the first section.
    pub intro: String,
    pub parts: Vec<FPart>,
}

const ORDER: [&str; 9] = [
    "lowk",
    "type",
    "id",
    "title",
    "tags",
    "allow_cloud",
    "default_template",
    "record_mic",
    "retranscribe",
];

impl FolderDoc {
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            tags: vec![],
            allow_cloud: true,
            default_template: None,
            mic: MicPreset::Inherit,
            retx: RetxPreset::Inherit,
            raw: Frontmatter::default(),
            intro: String::new(),
            parts: vec![
                FPart::Instructions("Instructions".into(), String::new()),
                FPart::Documents("Documents".into(), vec![]),
            ],
        }
    }

    pub fn instructions(&self) -> &str {
        self.parts
            .iter()
            .find_map(|p| {
                if let FPart::Instructions(_, t) = p {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .unwrap_or("")
    }

    pub fn set_instructions(&mut self, text: &str) {
        let text = text.trim_matches('\n').to_string();
        for p in &mut self.parts {
            if let FPart::Instructions(_, t) = p {
                *t = text;
                return;
            }
        }
        self.parts
            .insert(0, FPart::Instructions("Instructions".into(), text));
    }

    pub fn docs(&self) -> &[String] {
        self.parts
            .iter()
            .find_map(|p| {
                if let FPart::Documents(_, d) = p {
                    Some(d.as_slice())
                } else {
                    None
                }
            })
            .unwrap_or(&[])
    }

    pub fn set_docs(&mut self, docs: Vec<String>) {
        for p in &mut self.parts {
            if let FPart::Documents(_, d) = p {
                *d = docs;
                return;
            }
        }
        self.parts.push(FPart::Documents("Documents".into(), docs));
    }
}

fn parse_doc_list(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|l| {
            l.trim_start()
                .strip_prefix("- ")
                .or_else(|| l.trim_start().strip_prefix("* "))
        })
        .map(|l| {
            let l = l.trim();
            l.strip_prefix("[[")
                .and_then(|x| x.strip_suffix("]]"))
                .unwrap_or(l)
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

pub fn parse(md: &str) -> FolderDoc {
    let md = fm::normalize_eol(md);
    let (front, body) = fm::split(&md);
    let front = front.unwrap_or_default();
    let get = |k: &str| front.get(k).map(fm::scalar).filter(|s| !s.is_empty());
    let mut doc = FolderDoc::new(&get("id").unwrap_or_default(), &get("title").unwrap_or_default());
    doc.tags = front.get("tags").map(fm::list).unwrap_or_default();
    doc.allow_cloud = get("allow_cloud").map(|v| v != "false").unwrap_or(true);
    doc.default_template = get("default_template");
    doc.mic = match get("record_mic").as_deref() {
        Some("on" | "true") => MicPreset::On,
        Some("off" | "false") => MicPreset::Off,
        Some("auto") => MicPreset::Auto,
        _ => MicPreset::Inherit,
    };
    doc.retx = match get("retranscribe").as_deref() {
        Some("on" | "true") => RetxPreset::On,
        Some("off" | "false") => RetxPreset::Off,
        _ => RetxPreset::Inherit,
    };
    doc.raw = front;

    let mut intro: Vec<&str> = vec![];
    let mut sections: Vec<(String, Vec<&str>)> = vec![];
    let mut fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            fence = !fence;
        }
        if !fence {
            if let Some(h) = line.strip_prefix("## ") {
                sections.push((h.trim().to_string(), vec![]));
                continue;
            }
        }
        match sections.last_mut() {
            Some((_, l)) => l.push(line),
            None => intro.push(line),
        }
    }
    doc.intro = intro.join("\n").trim_matches('\n').to_string();
    doc.parts.clear();
    let (mut si, mut sd) = (false, false);
    for (h, lines) in sections {
        let text = lines.join("\n").trim_matches('\n').to_string();
        match h.to_lowercase().as_str() {
            "instructions" | "инструкции" if !si => {
                si = true;
                doc.parts.push(FPart::Instructions(h, text));
            }
            "documents" | "документы" if !sd => {
                sd = true;
                doc.parts.push(FPart::Documents(h, parse_doc_list(&text)));
            }
            _ => doc.parts.push(FPart::Other(h, text)),
        }
    }
    doc
}

pub fn render(doc: &FolderDoc) -> String {
    let mut f = doc.raw.clone();
    let q = |v: &str| fm::quote(v);
    f.set_ordered("lowk", "1".into(), &ORDER);
    f.set_ordered("type", "folder".into(), &ORDER);
    f.set_ordered("id", q(&doc.id), &ORDER);
    f.set_ordered("title", q(&doc.title), &ORDER);
    f.set_ordered("tags", fm::fmt_list(&doc.tags), &ORDER);
    f.set_ordered("allow_cloud", doc.allow_cloud.to_string(), &ORDER);
    match &doc.default_template {
        Some(t) => f.set_ordered("default_template", q(t), &ORDER),
        None => f.remove("default_template"),
    }
    match doc.mic {
        MicPreset::On => f.set_ordered("record_mic", "on".into(), &ORDER),
        MicPreset::Off => f.set_ordered("record_mic", "off".into(), &ORDER),
        MicPreset::Auto => f.set_ordered("record_mic", "auto".into(), &ORDER),
        MicPreset::Inherit => f.remove("record_mic"),
    }
    match doc.retx {
        RetxPreset::On => f.set_ordered("retranscribe", "on".into(), &ORDER),
        RetxPreset::Off => f.set_ordered("retranscribe", "off".into(), &ORDER),
        RetxPreset::Inherit => f.remove("retranscribe"),
    }
    let mut chunks = vec![f.render().trim_end().to_string()];
    if !doc.intro.is_empty() {
        chunks.push(doc.intro.clone());
    }
    for p in &doc.parts {
        let (h, body) = match p {
            FPart::Instructions(h, b) | FPart::Other(h, b) => (h, b.clone()),
            FPart::Documents(h, d) => (
                h,
                d.iter()
                    .map(|x| format!("- [[{x}]]"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        };
        chunks.push(if body.is_empty() {
            format!("## {h}")
        } else {
            format!("## {h}\n\n{body}")
        });
    }
    let mut s = chunks.join("\n\n");
    s.push('\n');
    s
}
