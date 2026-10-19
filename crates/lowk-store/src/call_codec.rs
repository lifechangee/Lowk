//! Call `.md` codec (architecture section 4.2).
//!
//! `parse` is total: any text becomes a [`CallDoc`] and `render` writes it back.
//! Rendering a parsed canonical file reproduces it byte for byte; hand edits
//! are normalised (spacing) but never lost: unknown frontmatter keys, unknown
//! sections and free text inside the transcript are preserved.
use crate::fm::{self, Frontmatter};
use lowk_types::fmt_offset;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Speaker {
    Me,
    System,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QaBlock {
    pub id: String,
    pub at_s: u32,
    pub template: String,
    pub model: String,
    pub range: String,
    /// Header attributes this version does not know, kept in order.
    pub extra_attrs: Vec<(String, String)>,
    pub question: String,
    pub answer: String,
    pub my_answer: Option<(u32, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageBlock {
    pub rel_path: String,
    pub at_s: u32,
    pub source: String,
    pub caption: String,
    /// False for a bare `![[...]]` embed without the `lowk-image` callout.
    pub callout: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Turn {
        at_s: u32,
        speaker: Speaker,
        text: String,
    },
    Qa(QaBlock),
    Image(ImageBlock),
    /// Anything we do not understand, preserved verbatim.
    Raw(String),
}

/// Document sections in file order. `Other` keeps user sections intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    /// Heading text as written, body.
    Summary(String, String),
    Transcript(String, Vec<Block>),
    Other(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallFm {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub started: String,
    pub ended: Option<String>,
    pub duration_s: Option<u64>,
    pub languages: Vec<String>,
    pub tags: Vec<String>,
    pub source_me: Option<String>,
    pub source_system: Option<String>,
    pub stt_engine: Option<String>,
    pub stt_mode: Option<String>,
    pub speaker_me: String,
    pub speaker_system: String,
    /// Extra speakers `(key, label)`, room for diarization.
    pub speaker_extra: Vec<(String, String)>,
    pub audio: String,
    /// Original frontmatter; unknown keys and key order come from here.
    pub raw: Frontmatter,
}

impl CallFm {
    pub fn new(id: &str, title: &str, started: &str) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            folder: String::new(),
            started: started.into(),
            ended: None,
            duration_s: None,
            languages: vec![],
            tags: vec![],
            source_me: None,
            source_system: None,
            stt_engine: None,
            stt_mode: None,
            speaker_me: "Я".into(),
            speaker_system: "Система".into(),
            speaker_extra: vec![],
            audio: "none".into(),
            raw: Frontmatter::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallDoc {
    pub fm: CallFm,
    /// The `# H1` line; falls back to the frontmatter title.
    pub heading: String,
    /// Text between the H1 and the first `##` section.
    pub intro: String,
    pub parts: Vec<Part>,
}

impl CallDoc {
    pub fn new(fm: CallFm) -> Self {
        let heading = fm.title.clone();
        Self {
            fm,
            heading,
            intro: String::new(),
            parts: vec![
                Part::Summary("Summary".into(), String::new()),
                Part::Transcript("Transcript".into(), vec![]),
            ],
        }
    }

    pub fn blocks(&self) -> &[Block] {
        self.parts
            .iter()
            .find_map(|p| {
                if let Part::Transcript(_, b) = p {
                    Some(b.as_slice())
                } else {
                    None
                }
            })
            .unwrap_or(&[])
    }

    pub fn blocks_mut(&mut self) -> &mut Vec<Block> {
        let i = match self.parts.iter().position(|p| matches!(p, Part::Transcript(..))) {
            Some(i) => i,
            None => {
                self.parts.push(Part::Transcript("Transcript".into(), vec![]));
                self.parts.len() - 1
            }
        };
        match &mut self.parts[i] {
            Part::Transcript(_, b) => b,
            _ => unreachable!("index points at a transcript part"),
        }
    }

    pub fn summary(&self) -> &str {
        self.parts
            .iter()
            .find_map(|p| {
                if let Part::Summary(_, b) = p {
                    Some(b.as_str())
                } else {
                    None
                }
            })
            .unwrap_or("")
    }

    pub fn set_summary(&mut self, text: &str) {
        for p in &mut self.parts {
            if let Part::Summary(_, b) = p {
                *b = text.trim_matches('\n').to_string();
                return;
            }
        }
        self.parts.insert(
            0,
            Part::Summary("Summary".into(), text.trim_matches('\n').to_string()),
        );
    }
}

// ---------------------------------------------------------------------------
// parse

pub fn parse_ts(s: &str) -> Option<u32> {
    let mut it = s.split(':');
    let (h, m, sec) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || m.len() != 2 || sec.len() != 2 || h.is_empty() {
        return None;
    }
    let ok = |x: &str| x.chars().all(|c| c.is_ascii_digit());
    if !(ok(h) && ok(m) && ok(sec)) {
        return None;
    }
    Some(h.parse::<u32>().ok()?.checked_mul(3600)? + m.parse::<u32>().ok()? * 60 + sec.parse::<u32>().ok()?)
}

fn fmt_ts(s: u32) -> String {
    fmt_offset(s as u64)
}

/// Parse a call document. Never fails.
pub fn parse(md: &str) -> CallDoc {
    let md = fm::normalize_eol(md);
    let (front, body) = fm::split(&md);
    let front = front.unwrap_or_default();
    let get = |k: &str| front.get(k).map(fm::scalar).filter(|s| !s.is_empty());
    let mut cf = CallFm::new(
        &get("id").unwrap_or_default(),
        &get("title").unwrap_or_default(),
        &get("started").unwrap_or_default(),
    );
    cf.folder = get("folder").unwrap_or_default();
    cf.ended = get("ended");
    cf.duration_s = get("duration_s").and_then(|s| s.parse().ok());
    cf.languages = front.get("languages").map(fm::list).unwrap_or_default();
    cf.tags = front.get("tags").map(fm::list).unwrap_or_default();
    for (k, v) in front.get("sources").map(fm::flow_map).unwrap_or_default() {
        match k.as_str() {
            "me" => cf.source_me = Some(v).filter(|s| !s.is_empty()),
            "system" => cf.source_system = Some(v).filter(|s| !s.is_empty()),
            _ => {}
        }
    }
    for (k, v) in front.get("stt").map(fm::flow_map).unwrap_or_default() {
        match k.as_str() {
            "engine" => cf.stt_engine = Some(v).filter(|s| !s.is_empty()),
            "mode" => cf.stt_mode = Some(v).filter(|s| !s.is_empty()),
            _ => {}
        }
    }
    for (k, v) in front.get("speakers").map(fm::flow_map).unwrap_or_default() {
        match k.as_str() {
            "me" => cf.speaker_me = v,
            "system" => cf.speaker_system = v,
            _ => cf.speaker_extra.push((k, v)),
        }
    }
    cf.audio = get("audio").unwrap_or_else(|| "none".into());
    cf.raw = front;

    // split body into H1 / intro / sections (fence-aware)
    let mut heading: Option<String> = None;
    let mut intro: Vec<&str> = vec![];
    let mut sections: Vec<(String, Vec<&str>)> = vec![];
    let mut fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            fence = !fence;
        }
        if !fence {
            if heading.is_none() && sections.is_empty() && line.starts_with("# ") {
                heading = Some(line[2..].trim().to_string());
                continue;
            }
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
    let join = |l: &[&str]| l.join("\n").trim_matches('\n').to_string();
    let mut parts = vec![];
    let (mut seen_sum, mut seen_tr) = (false, false);
    for (h, lines) in sections {
        let lower = h.to_lowercase();
        let text = join(&lines);
        if !seen_sum && matches!(lower.as_str(), "summary" | "саммари" | "резюме") {
            seen_sum = true;
            parts.push(Part::Summary(h, text));
        } else if !seen_tr && matches!(lower.as_str(), "transcript" | "транскрипт") {
            seen_tr = true;
            parts.push(Part::Transcript(h, parse_blocks(&text)));
        } else {
            parts.push(Part::Other(h, text));
        }
    }
    let mut doc = CallDoc {
        heading: heading.unwrap_or_else(|| cf.title.clone()),
        fm: cf,
        intro: join(&intro),
        parts,
    };
    resolve(&mut doc);
    doc
}

fn parse_turn(line: &str) -> Option<(u32, Speaker, String, String)> {
    let rest = line.strip_prefix('[')?;
    let (ts, rest) = rest.split_once(']')?;
    let at = parse_ts(ts)?;
    let rest = rest.strip_prefix(' ')?.strip_prefix("**")?;
    let (label, text) = rest.split_once(":**")?;
    let text = text.strip_prefix(' ').unwrap_or(text);
    Some((
        at,
        Speaker::Other(label.to_string()),
        text.to_string(),
        label.to_string(),
    ))
}

fn parse_blocks(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<Block> = vec![];
    let mut i = 0;
    let mut prev_blank = true;
    // Labels are resolved to Me/System by the caller via `resolve_speakers`.
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            prev_blank = true;
            i += 1;
            continue;
        }
        if line.starts_with("> [!lowk-qa]") {
            let mut j = i + 1;
            while j < lines.len() && lines[j].starts_with('>') {
                j += 1;
            }
            out.push(Block::Qa(parse_qa(&lines[i..j])));
            i = j;
        } else if let Some(rel) = line.strip_prefix("![[").and_then(|r| r.strip_suffix("]]")) {
            let mut b = ImageBlock {
                rel_path: rel.to_string(),
                at_s: 0,
                source: String::new(),
                caption: String::new(),
                callout: false,
            };
            let mut j = i + 1;
            if j < lines.len() && lines[j].starts_with("> [!lowk-image]") {
                b.callout = true;
                for (k, v) in attrs(&lines[j]["> [!lowk-image]".len()..]) {
                    match k.as_str() {
                        "at" => b.at_s = parse_ts(&v).unwrap_or(0),
                        "source" => b.source = v,
                        _ => {}
                    }
                }
                j += 1;
                let mut cap = vec![];
                while j < lines.len() && lines[j].starts_with('>') {
                    cap.push(strip_quote(lines[j]));
                    j += 1;
                }
                b.caption = cap.join("\n");
            }
            out.push(Block::Image(b));
            i = j;
        } else if let Some((at_s, speaker, text, _)) = parse_turn(line) {
            out.push(Block::Turn { at_s, speaker, text });
            i += 1;
        } else {
            match out.last_mut() {
                Some(Block::Turn { text, .. }) if !prev_blank => {
                    text.push('\n');
                    text.push_str(line);
                }
                Some(Block::Raw(r)) if !prev_blank => {
                    r.push('\n');
                    r.push_str(line);
                }
                _ => out.push(Block::Raw(line.to_string())),
            }
            i += 1;
        }
        prev_blank = false;
    }
    out
}

fn strip_quote(l: &str) -> String {
    l.strip_prefix("> ")
        .or_else(|| l.strip_prefix('>'))
        .unwrap_or(l)
        .to_string()
}

fn attrs(s: &str) -> Vec<(String, String)> {
    s.split_whitespace()
        .filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect()
}

fn parse_qa(lines: &[&str]) -> QaBlock {
    let mut q = QaBlock {
        id: String::new(),
        at_s: 0,
        template: String::new(),
        model: String::new(),
        range: String::new(),
        extra_attrs: vec![],
        question: String::new(),
        answer: String::new(),
        my_answer: None,
    };
    for (k, v) in attrs(&lines[0]["> [!lowk-qa]".len()..]) {
        match k.as_str() {
            "id" => q.id = v,
            "at" => q.at_s = parse_ts(&v).unwrap_or(0),
            "template" => q.template = v,
            "model" => q.model = v,
            "range" => q.range = v,
            _ => q.extra_attrs.push((k, v)),
        }
    }
    const MARKS: [(&str, u8); 6] = [
        ("**Вопрос:**", 0),
        ("**Подсказка ИИ:**", 1),
        ("**Мой ответ:**", 2),
        ("**Question:**", 0),
        ("**AI hint:**", 1),
        ("**My answer:**", 2),
    ];
    let mut fields: [Vec<String>; 3] = Default::default();
    let mut cur: Option<usize> = None;
    for l in &lines[1..] {
        let l = strip_quote(l);
        if let Some((m, idx)) = MARKS.iter().find(|(m, _)| l.starts_with(m)) {
            cur = Some(*idx as usize);
            fields[*idx as usize].push(l[m.len()..].trim_start().to_string());
        } else if let Some(c) = cur {
            fields[c].push(l);
        }
    }
    let [qs, ans, mine] = fields.map(|f| f.join("\n").trim_end().to_string());
    q.question = qs;
    q.answer = ans;
    if !mine.is_empty() {
        let (at, text) = match mine.strip_prefix('[').and_then(|r| r.split_once(']')) {
            Some((ts, rest)) if parse_ts(ts).is_some() => {
                (parse_ts(ts).unwrap_or(0), rest.trim_start().to_string())
            }
            _ => (q.at_s, mine),
        };
        q.my_answer = Some((at, text));
    }
    q
}

// ---------------------------------------------------------------------------
// render

fn quote_lines(s: &str) -> String {
    s.lines()
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ws_free(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join("_")
}

fn field(label: &str, text: &str) -> String {
    let mut it = text.lines();
    let first = it.next().unwrap_or("");
    let mut s = if first.is_empty() {
        format!("> {label}")
    } else {
        format!("> {label} {first}")
    };
    for l in it {
        s.push('\n');
        s.push_str(&if l.is_empty() {
            ">".to_string()
        } else {
            format!("> {l}")
        });
    }
    s
}

fn render_block(b: &Block, fm: &CallFm) -> String {
    match b {
        Block::Turn { at_s, speaker, text } => {
            let label = match speaker {
                Speaker::Me => &fm.speaker_me,
                Speaker::System => &fm.speaker_system,
                Speaker::Other(l) => l,
            };
            format!("[{}] **{label}:** {text}", fmt_ts(*at_s))
                .trim_end()
                .to_string()
        }
        Block::Qa(q) => {
            let mut h = format!("> [!lowk-qa] id={} at={}", ws_free(&q.id), fmt_ts(q.at_s));
            for (k, v) in [
                ("template", &q.template),
                ("model", &q.model),
                ("range", &q.range),
            ] {
                if !v.is_empty() {
                    h.push_str(&format!(" {k}={}", ws_free(v)));
                }
            }
            for (k, v) in &q.extra_attrs {
                h.push_str(&format!(" {k}={v}"));
            }
            let mut lines = vec![
                h,
                field("**Вопрос:**", &q.question),
                field("**Подсказка ИИ:**", &q.answer),
            ];
            if let Some((at, t)) = &q.my_answer {
                lines.push(field("**Мой ответ:**", &format!("[{}] {t}", fmt_ts(*at))));
            }
            lines.join("\n")
        }
        Block::Image(im) => {
            let mut s = format!("![[{}]]", im.rel_path);
            if im.callout {
                s.push_str(&format!("\n> [!lowk-image] at={}", fmt_ts(im.at_s)));
                if !im.source.is_empty() {
                    s.push_str(&format!(" source={}", ws_free(&im.source)));
                }
                if !im.caption.is_empty() {
                    s.push('\n');
                    s.push_str(&quote_lines(&im.caption));
                }
            }
            s
        }
        Block::Raw(r) => r.clone(),
    }
}

fn render_frontmatter(f: &CallFm) -> Frontmatter {
    let mut out = f.raw.clone();
    let s = |v: &str| fm::quote(v);
    // New keys land at their canonical position; existing keys keep theirs.
    const ORDER: [&str; 14] = [
        "lowk",
        "type",
        "id",
        "title",
        "folder",
        "started",
        "ended",
        "duration_s",
        "languages",
        "tags",
        "sources",
        "stt",
        "speakers",
        "audio",
    ];
    out.set_ordered("lowk", "1".into(), &ORDER);
    out.set_ordered("type", "call".into(), &ORDER);
    out.set_ordered("id", s(&f.id), &ORDER);
    out.set_ordered("title", s(&f.title), &ORDER);
    out.set_ordered("folder", s(&f.folder), &ORDER);
    out.set_ordered("started", s(&f.started), &ORDER);
    match &f.ended {
        Some(e) => out.set_ordered("ended", s(e), &ORDER),
        None => out.remove("ended"),
    }
    match f.duration_s {
        Some(d) => out.set_ordered("duration_s", d.to_string(), &ORDER),
        None => out.remove("duration_s"),
    }
    out.set_ordered("languages", fm::fmt_list(&f.languages), &ORDER);
    out.set_ordered("tags", fm::fmt_list(&f.tags), &ORDER);
    let mut src = vec![];
    if let Some(v) = &f.source_me {
        src.push(("me".to_string(), v.clone()));
    }
    if let Some(v) = &f.source_system {
        src.push(("system".to_string(), v.clone()));
    }
    out.set_ordered("sources", fm::fmt_flow_map(&src), &ORDER);
    let mut stt = vec![];
    if let Some(v) = &f.stt_engine {
        stt.push(("engine".to_string(), v.clone()));
    }
    if let Some(v) = &f.stt_mode {
        stt.push(("mode".to_string(), v.clone()));
    }
    if stt.is_empty() {
        out.remove("stt");
    } else {
        out.set_ordered("stt", fm::fmt_flow_map(&stt), &ORDER);
    }
    let mut sp = vec![
        ("me".to_string(), f.speaker_me.clone()),
        ("system".to_string(), f.speaker_system.clone()),
    ];
    sp.extend(f.speaker_extra.iter().cloned());
    out.set_ordered("speakers", fm::fmt_flow_map(&sp), &ORDER);
    out.set_ordered("audio", s(&f.audio), &ORDER);
    out
}

/// Resolve speaker labels (`Other(label)` straight from the parser) to Me/System
/// using the frontmatter `speakers` table.
fn resolve(doc: &mut CallDoc) {
    let (me, sys) = (doc.fm.speaker_me.clone(), doc.fm.speaker_system.clone());
    for p in &mut doc.parts {
        if let Part::Transcript(_, blocks) = p {
            for b in blocks {
                if let Block::Turn { speaker, .. } = b {
                    if let Speaker::Other(l) = speaker {
                        if *l == me {
                            *speaker = Speaker::Me;
                        } else if *l == sys {
                            *speaker = Speaker::System;
                        }
                    }
                }
            }
        }
    }
}

pub fn render(doc: &CallDoc) -> String {
    let mut chunks: Vec<String> = vec![render_frontmatter(&doc.fm).render().trim_end().to_string()];
    let h = if doc.heading.is_empty() {
        &doc.fm.title
    } else {
        &doc.heading
    };
    chunks.push(format!("# {h}"));
    if !doc.intro.is_empty() {
        chunks.push(doc.intro.clone());
    }
    let mut parts = doc.parts.clone();
    if !parts.iter().any(|p| matches!(p, Part::Summary(..))) {
        let at = parts
            .iter()
            .position(|p| matches!(p, Part::Transcript(..)))
            .unwrap_or(parts.len());
        parts.insert(at, Part::Summary("Summary".into(), String::new()));
    }
    if !parts.iter().any(|p| matches!(p, Part::Transcript(..))) {
        parts.push(Part::Transcript("Transcript".into(), vec![]));
    }
    for p in &parts {
        let (head, body) = match p {
            Part::Summary(h, b) | Part::Other(h, b) => (h, b.clone()),
            Part::Transcript(h, blocks) => (
                h,
                blocks
                    .iter()
                    .map(|b| render_block(b, &doc.fm))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            ),
        };
        chunks.push(if body.is_empty() {
            format!("## {head}")
        } else {
            format!("## {head}\n\n{body}")
        });
    }
    let mut s = chunks.join("\n\n");
    s.push('\n');
    s
}
