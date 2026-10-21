//! Markdown (call transcripts and converted documents) -> chunks.
//!
//! * Transcript: windows of 100-300 tokens cut on turn boundaries (or a pause), one turn of
//!   overlap, never across a `lowk-image` / `lowk-qa` callout (those become their own chunks).
//! * Documents and user sections: by headings, 300-500 tokens, tables kept whole, `<!-- p:N -->`
//!   markers become the page of the chunk.
//! * Chunk ids are `blake3(file id, text, n-th duplicate)`: byte-stable across reindexes and not
//!   shifted by an edit elsewhere in the file.
use crate::text::estimate_tokens;
use lowk_types::{ChunkKind, IndexItemKind};
use std::collections::HashMap;

/// Bump to force a re-chunk of everything.
pub const CHUNKER_VER: u32 = 1;

const SPEECH_MAX: u32 = 300;
const SPEECH_MIN: u32 = 100;
const SPEECH_PAUSE_S: u32 = 30;
const DOC_MAX: u32 = 500;
const PAGE_FLUSH_MIN: u32 = 150;

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub cid: String,
    pub ord: u32,
    pub kind: ChunkKind,
    pub start_s: Option<u32>,
    pub page: Option<u32>,
    pub heading_path: Vec<String>,
    /// Text shown to users and sent to the model (speech keeps `Label: ` prefixes).
    pub text: String,
    /// Text that is stemmed into FTS (no speaker labels).
    pub index_text: String,
    pub n_tokens: u32,
}

struct Turn {
    t: Option<u32>,
    label: String,
    text: String,
    tokens: u32,
}

struct Raw {
    kind: ChunkKind,
    start_s: Option<u32>,
    page: Option<u32>,
    heading_path: Vec<String>,
    text: String,
    index_text: String,
}

pub fn chunk_markdown(file_id: &str, kind: IndexItemKind, md: &str) -> Vec<Chunk> {
    let mut raws = Vec::new();
    Parser::new(kind, &mut raws).run(strip_frontmatter(md));
    let mut seen: HashMap<String, u32> = HashMap::new();
    raws.into_iter()
        .enumerate()
        .map(|(ord, r)| {
            let dup = seen.entry(r.text.clone()).or_insert(0);
            let mut h = blake3::Hasher::new();
            h.update(file_id.as_bytes());
            h.update(&[0]);
            h.update(r.text.as_bytes());
            h.update(&[0]);
            h.update(&dup.to_le_bytes());
            *dup += 1;
            let hex = h.finalize().to_hex();
            let n_tokens = estimate_tokens(&r.text);
            Chunk {
                cid: format!("c{}", &hex.as_str()[..16]),
                ord: ord as u32,
                kind: r.kind,
                start_s: r.start_s,
                page: r.page,
                heading_path: r.heading_path,
                text: r.text,
                index_text: r.index_text,
                n_tokens,
            }
        })
        .collect()
}

fn strip_frontmatter(md: &str) -> &str {
    let t = md.trim_start_matches('\u{feff}');
    if let Some(rest) = t.strip_prefix("---\n") {
        if let Some(i) = rest.find("\n---") {
            let after = &rest[i + 4..];
            return after.split_once('\n').map(|x| x.1).unwrap_or("");
        }
    }
    t
}

/// `HH:MM:SS` (hours may exceed two digits) or `MM:SS`.
pub fn parse_hms(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    let n: Vec<u32> = parts.iter().map(|p| p.parse().ok()).collect::<Option<_>>()?;
    match n.as_slice() {
        [h, m, s] => Some(h * 3600 + m * 60 + s),
        [m, s] => Some(m * 60 + s),
        _ => None,
    }
}

fn parse_turn(line: &str) -> Option<(u32, String, String)> {
    let rest = line.strip_prefix('[')?;
    let (ts, rest) = rest.split_once(']')?;
    let t = parse_hms(ts)?;
    let rest = rest.trim_start().strip_prefix("**")?;
    let (label, text) = rest.split_once(":**")?;
    Some((t, label.trim().to_string(), text.trim().to_string()))
}

fn page_marker(line: &str) -> Option<u32> {
    let t = line.trim();
    let inner = t.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    inner.strip_prefix("p:")?.trim().parse().ok()
}

fn is_transcript_heading(h: &str) -> bool {
    matches!(
        h.trim().to_lowercase().as_str(),
        "transcript" | "транскрипт" | "расшифровка"
    )
}

struct Parser<'a> {
    kind: IndexItemKind,
    out: &'a mut Vec<Raw>,
    headings: Vec<(usize, String)>,
    page: Option<u32>,
    in_transcript: bool,
    turns: Vec<Turn>,
    turn_tokens: u32,
    paras: Vec<String>,
    para_tokens: u32,
    para_page: Option<u32>,
}

impl<'a> Parser<'a> {
    fn new(kind: IndexItemKind, out: &'a mut Vec<Raw>) -> Self {
        Self {
            kind,
            out,
            headings: vec![],
            page: None,
            in_transcript: false,
            turns: vec![],
            turn_tokens: 0,
            paras: vec![],
            para_tokens: 0,
            para_page: None,
        }
    }

    fn path(&self) -> Vec<String> {
        self.headings.iter().map(|h| h.1.clone()).collect()
    }

    fn run(&mut self, md: &str) {
        let lines: Vec<&str> = md.lines().collect();
        let mut i = 0;
        let mut para: Vec<&str> = vec![];
        while i < lines.len() {
            let line = lines[i];
            let t = line.trim_end();
            // --- structure that ends a paragraph
            if t.trim().is_empty() {
                self.end_para(&mut para);
                i += 1;
                continue;
            }
            if let Some(p) = page_marker(t) {
                self.end_para(&mut para);
                if self.para_tokens >= PAGE_FLUSH_MIN {
                    self.flush_doc();
                }
                self.page = Some(p);
                i += 1;
                continue;
            }
            if let Some((lvl, h)) = heading(t) {
                self.end_para(&mut para);
                self.flush_speech();
                self.flush_doc();
                while self.headings.last().is_some_and(|x| x.0 >= lvl) {
                    self.headings.pop();
                }
                if lvl > 1 || self.kind == IndexItemKind::Doc {
                    self.headings.push((lvl, h.to_string()));
                } else if lvl == 1 {
                    // `# Title` of a call repeats the title; keep it out of the path.
                }
                self.in_transcript = self.kind == IndexItemKind::Call && is_transcript_heading(h);
                i += 1;
                continue;
            }
            if t.trim_start().starts_with("![[") {
                self.end_para(&mut para);
                i += 1;
                continue;
            }
            if let Some(rest) = t.trim_start().strip_prefix("> [!") {
                // callout: header line + following `>` lines
                self.end_para(&mut para);
                let (ckind, attrs) = rest.split_once(']').unwrap_or((rest, ""));
                let ckind = ckind.trim().to_lowercase();
                let mut body = vec![];
                let mut j = i + 1;
                while j < lines.len() && lines[j].trim_start().starts_with('>') {
                    body.push(lines[j].trim_start().trim_start_matches('>').trim().to_string());
                    j += 1;
                }
                self.callout(&ckind, attrs, &body);
                i = j;
                continue;
            }
            if self.kind == IndexItemKind::Call {
                if let Some((ts, label, text)) = parse_turn(t.trim_start()) {
                    self.end_para(&mut para);
                    self.add_turn(Some(ts), label, text);
                    i += 1;
                    continue;
                }
                if self.in_transcript {
                    // continuation of the previous turn (hand edits)
                    if let Some(last) = self.turns.last_mut() {
                        last.text.push(' ');
                        last.text.push_str(t.trim());
                        last.tokens = estimate_tokens(&last.text);
                        self.turn_tokens = self.turns.iter().map(|x| x.tokens).sum();
                    } else {
                        self.add_turn(None, String::new(), t.trim().to_string());
                    }
                    i += 1;
                    continue;
                }
            }
            if t.trim_start().starts_with('|') {
                self.end_para(&mut para);
                let mut j = i;
                let mut tbl = vec![];
                while j < lines.len() && lines[j].trim_start().starts_with('|') {
                    tbl.push(lines[j].trim_end());
                    j += 1;
                }
                self.table(&tbl);
                i = j;
                continue;
            }
            para.push(t);
            i += 1;
        }
        self.end_para(&mut para);
        self.flush_speech();
        self.flush_doc();
    }

    fn callout(&mut self, ckind: &str, attrs: &str, body: &[String]) {
        let at = attrs
            .split_whitespace()
            .find_map(|a| a.strip_prefix("at="))
            .and_then(parse_hms);
        let text = body
            .iter()
            .map(|l| l.replace("**", ""))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        let kind = match ckind {
            "lowk-qa" => ChunkKind::Qa,
            "lowk-image" => ChunkKind::Image,
            _ => {
                // an ordinary callout is user text
                for l in body {
                    self.add_para(l.clone());
                }
                return;
            }
        };
        self.flush_speech();
        self.flush_doc();
        if text.is_empty() {
            return;
        }
        let path = self.path();
        self.out.push(Raw {
            kind,
            start_s: at,
            page: None,
            heading_path: path,
            index_text: text.clone(),
            text,
        });
    }

    fn add_turn(&mut self, t: Option<u32>, label: String, text: String) {
        if text.is_empty() {
            return;
        }
        // an overlong monologue is cut into sentence groups
        for piece in split_long(&text, SPEECH_MAX) {
            let tokens = estimate_tokens(&piece) + estimate_tokens(&label);
            let pause = match (self.turns.last().and_then(|x| x.t), t) {
                (Some(a), Some(b)) => b.saturating_sub(a) > SPEECH_PAUSE_S,
                _ => false,
            };
            if !self.turns.is_empty() {
                if pause && self.turn_tokens >= PAGE_FLUSH_MIN {
                    self.flush_speech();
                } else if self.turn_tokens + tokens > SPEECH_MAX && self.turn_tokens >= SPEECH_MIN {
                    let keep = self
                        .turns
                        .last()
                        .filter(|l| l.tokens <= SPEECH_MIN)
                        .map(|l| Turn {
                            t: l.t,
                            label: l.label.clone(),
                            text: l.text.clone(),
                            tokens: l.tokens,
                        });
                    self.flush_speech();
                    if let Some(k) = keep {
                        self.turn_tokens = k.tokens;
                        self.turns.push(k);
                    }
                }
            }
            self.turn_tokens += tokens;
            self.turns.push(Turn {
                t,
                label: label.clone(),
                text: piece,
                tokens,
            });
        }
    }

    fn flush_speech(&mut self) {
        if self.turns.is_empty() {
            return;
        }
        let turns = std::mem::take(&mut self.turns);
        self.turn_tokens = 0;
        let path = self.path();
        let text = turns
            .iter()
            .map(|t| {
                if t.label.is_empty() {
                    t.text.clone()
                } else {
                    format!("{}: {}", t.label, t.text)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let index_text = turns
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.out.push(Raw {
            kind: ChunkKind::Speech,
            start_s: turns.iter().find_map(|t| t.t),
            page: None,
            heading_path: path,
            text,
            index_text,
        });
    }

    fn end_para(&mut self, para: &mut Vec<&str>) {
        if para.is_empty() {
            return;
        }
        let p = para.join("\n");
        para.clear();
        self.add_para(p);
    }

    fn add_para(&mut self, p: String) {
        let p = p.trim().to_string();
        if p.is_empty() {
            return;
        }
        for piece in split_long(&p, DOC_MAX - 50) {
            let tk = estimate_tokens(&piece);
            if self.para_tokens + tk > DOC_MAX && self.para_tokens > 0 {
                self.flush_doc();
            }
            if self.paras.is_empty() {
                self.para_page = self.page;
            }
            self.paras.push(piece);
            self.para_tokens += tk;
            if self.para_tokens >= DOC_MAX - 60 {
                self.flush_doc();
            }
        }
    }

    fn table(&mut self, rows: &[&str]) {
        let total: u32 = rows.iter().map(|r| estimate_tokens(r)).sum();
        if total <= DOC_MAX {
            if self.para_tokens + total > DOC_MAX {
                self.flush_doc();
            }
            if self.paras.is_empty() {
                self.para_page = self.page;
            }
            self.paras.push(rows.join("\n"));
            self.para_tokens += total;
            return;
        }
        // keep the header (first two rows) on every part
        let head: Vec<&str> = rows.iter().take(2).copied().collect();
        let head_tk: u32 = head.iter().map(|r| estimate_tokens(r)).sum();
        self.flush_doc();
        let mut cur: Vec<&str> = head.clone();
        let mut tk = head_tk;
        for r in rows.iter().skip(2) {
            let rt = estimate_tokens(r);
            if tk + rt > DOC_MAX && cur.len() > head.len() {
                self.paras.push(cur.join("\n"));
                self.para_tokens = tk;
                self.para_page = self.page;
                self.flush_doc();
                cur = head.clone();
                tk = head_tk;
            }
            cur.push(r);
            tk += rt;
        }
        if cur.len() > head.len() {
            self.paras.push(cur.join("\n"));
            self.para_tokens = tk;
            self.para_page = self.page;
            self.flush_doc();
        }
    }

    fn flush_doc(&mut self) {
        if self.paras.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.paras).join("\n\n");
        self.para_tokens = 0;
        let path = self.path();
        self.out.push(Raw {
            kind: ChunkKind::Doc,
            start_s: None,
            page: self.para_page,
            heading_path: path,
            index_text: text.clone(),
            text,
        });
    }
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    let n = t.chars().take_while(|c| *c == '#').count();
    if n == 0 || n > 6 {
        return None;
    }
    let rest = t[n..].strip_prefix(' ')?;
    Some((n, rest.trim()))
}

/// Splits `text` into pieces of at most `max` tokens, on sentence ends, then on words.
fn split_long(text: &str, max: u32) -> Vec<String> {
    if estimate_tokens(text) <= max {
        return vec![text.to_string()];
    }
    let mut out = vec![];
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.trim().is_empty() {
            out.push(cur.trim().to_string());
        }
        cur.clear();
    };
    let mut sentence = String::new();
    let mut sentences = vec![];
    for c in text.chars() {
        sentence.push(c);
        if matches!(c, '.' | '!' | '?' | '\n') {
            sentences.push(std::mem::take(&mut sentence));
        }
    }
    if !sentence.is_empty() {
        sentences.push(sentence);
    }
    for s in sentences {
        if estimate_tokens(&s) > max {
            flush(&mut cur, &mut out);
            for w in s.split_whitespace() {
                if estimate_tokens(&cur) + estimate_tokens(w) > max {
                    flush(&mut cur, &mut out);
                }
                if !cur.is_empty() {
                    cur.push(' ');
                }
                cur.push_str(w);
            }
            continue;
        }
        if estimate_tokens(&cur) + estimate_tokens(&s) > max {
            flush(&mut cur, &mut out);
        }
        cur.push_str(&s);
    }
    flush(&mut cur, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call_md() -> String {
        let mut s = String::from("# Weekly\n\n## Summary\n\n## Transcript\n\n");
        for i in 0..40 {
            s.push_str(&format!(
                "[00:{:02}:{:02}] **Система:** Это реплика номер {i}, мы обсуждаем бюджет проекта и сроки поставки оборудования для клиента.\n",
                i / 6,
                (i % 6) * 10
            ));
        }
        s.push_str("\n> [!lowk-qa] id=X at=00:07:00 template=brief\n> **Вопрос:** Что такое EBITDA?\n> **Подсказка ИИ:** Прибыль до вычета процентов.\n\n");
        s.push_str("![[a.assets/x.png]]\n> [!lowk-image] at=00:08:00 source=manual\n> Слайд Roadmap Q4\n\n");
        s.push_str("[00:08:30] **Me:** Спасибо.\n");
        s
    }

    #[test]
    fn transcript_windows_and_callouts() {
        let ch = chunk_markdown("F1", IndexItemKind::Call, &call_md());
        let speech: Vec<_> = ch.iter().filter(|c| c.kind == ChunkKind::Speech).collect();
        assert!(speech.len() >= 3, "{}", speech.len());
        for c in &speech {
            assert!(c.n_tokens <= 420, "{}", c.n_tokens);
            assert!(c.start_s.is_some());
            assert!(!c.text.contains("Подсказка"));
        }
        assert_eq!(ch.iter().filter(|c| c.kind == ChunkKind::Qa).count(), 1);
        let img = ch.iter().find(|c| c.kind == ChunkKind::Image).unwrap();
        assert_eq!(img.start_s, Some(480));
        assert!(img.text.contains("Roadmap"));
        // the last turn comes after the image and is its own window
        assert!(ch.last().unwrap().text.contains("Спасибо"));
    }

    #[test]
    fn doc_pages_headings_tables() {
        let mut md =
            String::from("<!-- p:1 -->\n# Intro\n\nПервый абзац о системе.\n\n<!-- p:2 -->\n## Scope\n\n");
        for i in 0..60 {
            md.push_str(&format!(
                "Sentence number {i} explains the scope of the integration in some detail.\n"
            ));
        }
        md.push_str("\n<!-- p:3 -->\n## Data\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
        let ch = chunk_markdown("D1", IndexItemKind::Doc, &md);
        assert!(ch.iter().all(|c| c.kind == ChunkKind::Doc));
        assert_eq!(ch[0].page, Some(1));
        assert_eq!(ch[0].heading_path, vec!["Intro"]);
        let scope: Vec<_> = ch
            .iter()
            .filter(|c| c.heading_path == vec!["Intro", "Scope"])
            .collect();
        assert!(scope.iter().all(|c| c.n_tokens <= DOC_MAX + 20));
        assert!(scope[0].page == Some(2));
        let last = ch.last().unwrap();
        assert_eq!(last.page, Some(3));
        assert!(last.text.contains("| 1 | 2 |"));
    }

    #[test]
    fn ids_are_byte_stable_and_unique() {
        let a = chunk_markdown("F1", IndexItemKind::Call, &call_md());
        let b = chunk_markdown("F1", IndexItemKind::Call, &call_md());
        assert_eq!(a, b);
        let mut ids: Vec<_> = a.iter().map(|c| c.cid.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), a.len());
        let other = chunk_markdown("F2", IndexItemKind::Call, &call_md());
        assert_ne!(a[0].cid, other[0].cid);
    }

    #[test]
    fn inserting_early_text_keeps_later_ids() {
        let a = chunk_markdown("D", IndexItemKind::Doc, "# A\n\nalpha para\n\n# B\n\nbeta para\n");
        let b = chunk_markdown(
            "D",
            IndexItemKind::Doc,
            "# A\n\nalpha para\n\nnew para\n\n# B\n\nbeta para\n",
        );
        let beta_a = a.iter().find(|c| c.text.contains("beta")).unwrap();
        let beta_b = b.iter().find(|c| c.text.contains("beta")).unwrap();
        assert_eq!(beta_a.cid, beta_b.cid);
    }

    #[test]
    fn frontmatter_and_garbage_do_not_panic() {
        let ch = chunk_markdown("X", IndexItemKind::Doc, "---\nid: 1\n---\n\n\n\n");
        assert!(ch.is_empty());
        let ch = chunk_markdown("X", IndexItemKind::Call, "[xx:yy] **a:** b\n> [!lowk-qa\n>\n");
        let _ = ch;
    }
}
