//! Minimal order-preserving YAML-frontmatter handling.
//!
//! We only need flat `key: value` pairs where values are scalars, flow lists
//! (`[a, b]`) or flow maps (`{ me: "x" }`). Everything else (block lists,
//! nested maps, comments) is kept as raw text so unknown keys survive a
//! rewrite byte for byte.

/// Ordered `key -> raw value` pairs. A key of `""` holds a verbatim line
/// (comment / blank) that is written back unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub entries: Vec<(String, String)>,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Replace the raw value of `key`; when absent, insert it after the last
    /// entry whose key precedes it in `order` (append when none does).
    pub fn set_ordered(&mut self, key: &str, raw: String, order: &[&str]) {
        if let Some(e) = self.entries.iter_mut().find(|(k, _)| k == key) {
            e.1 = raw;
            return;
        }
        let pos = order.iter().position(|k| *k == key).unwrap_or(order.len());
        let at = self
            .entries
            .iter()
            .rposition(|(k, _)| order.iter().position(|o| o == k).is_some_and(|p| p < pos))
            .map_or(0, |i| i + 1);
        self.entries
            .insert(at.min(self.entries.len()), (key.to_string(), raw));
    }

    pub fn remove(&mut self, key: &str) {
        self.entries.retain(|(k, _)| k != key);
    }

    pub fn render(&self) -> String {
        let mut s = String::from("---\n");
        for (k, v) in &self.entries {
            if k.is_empty() {
                s.push_str(v);
            } else if v.is_empty() {
                s.push_str(k);
                s.push(':');
            } else if v.starts_with('\n') {
                s.push_str(k);
                s.push(':');
                s.push_str(v);
            } else {
                s.push_str(k);
                s.push_str(": ");
                s.push_str(v);
            }
            s.push('\n');
        }
        s.push_str("---\n");
        s
    }
}

/// Normalise line endings so the codec only ever sees `\n`.
pub fn normalize_eol(s: &str) -> String {
    let s = s.strip_prefix('\u{feff}').unwrap_or(s);
    if s.contains('\r') {
        s.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        s.to_string()
    }
}

/// Split a document into frontmatter and body. `md` must already be `\n`-normalised.
/// Returns `(None, md)` when there is no well-formed frontmatter block.
pub fn split(md: &str) -> (Option<Frontmatter>, &str) {
    let Some(rest) = md.strip_prefix("---\n") else {
        return (None, md);
    };
    let (block, body) = if let Some(i) = rest.find("\n---\n") {
        (&rest[..i + 1], &rest[i + 5..])
    } else if let Some(stripped) = rest.strip_suffix("\n---") {
        (stripped, "")
    } else if let Some(after) = rest.strip_prefix("---\n") {
        ("", after)
    } else if rest == "---" {
        ("", "")
    } else {
        return (None, md);
    };
    let mut fm = Frontmatter::default();
    for line in block.lines() {
        let is_key = !line.starts_with([' ', '\t', '-', '#'])
            && line.find(':').is_some_and(|i| {
                let k = &line[..i];
                !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            });
        if is_key {
            let i = line.find(':').unwrap_or(0);
            fm.entries
                .push((line[..i].to_string(), line[i + 1..].trim().to_string()));
        } else if let Some(last) = fm
            .entries
            .last_mut()
            .filter(|(k, _)| !k.is_empty() && !line.starts_with('#'))
        {
            last.1.push('\n');
            last.1.push_str(line);
        } else {
            fm.entries.push((String::new(), line.to_string()));
        }
    }
    (Some(fm), body)
}

fn strip_comment(s: &str) -> &str {
    let mut q: Option<char> = None;
    let mut prev_ws = true;
    for (i, c) in s.char_indices() {
        match q {
            Some(qc) => {
                if c == qc {
                    q = None;
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    q = Some(c);
                } else if c == '#' && prev_ws {
                    return s[..i].trim_end();
                }
            }
        }
        prev_ws = c.is_whitespace();
    }
    s.trim_end()
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let mut out = String::new();
        let mut it = s[1..s.len() - 1].chars();
        while let Some(c) = it.next() {
            if c == '\\' {
                match it.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(o) => out.push(o),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    } else if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        s[1..s.len() - 1].replace("''", "'")
    } else {
        s.to_string()
    }
}

/// Parse a scalar value (comment stripped, quotes removed).
pub fn scalar(raw: &str) -> String {
    let first = raw.lines().next().unwrap_or("");
    unquote(strip_comment(first))
}

/// Serialise a scalar, quoting only when plain style would be ambiguous.
pub fn quote(s: &str) -> String {
    let needs = s.is_empty()
        || s.starts_with(|c: char| "[{]}&*!|>'\"%@`,#-?: ".contains(c))
        || s.ends_with([' ', ':'])
        || s.contains(": ")
        || s.contains(" #")
        || s.contains(['\n', '\t', '\\'])
        || matches!(s, "null" | "~" | "true" | "false");
    if needs {
        let mut o = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                '\t' => o.push_str("\\t"),
                c => o.push(c),
            }
        }
        o.push('"');
        o
    } else {
        s.to_string()
    }
}

/// Item quoting inside flow collections additionally protects `, [ ] { }`.
fn quote_item(s: &str) -> String {
    if s.contains([',', '[', ']', '{', '}']) {
        let mut o = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                c => o.push(c),
            }
        }
        o.push('"');
        o
    } else {
        quote(s)
    }
}

fn split_top(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q: Option<char> = None;
    let mut depth = 0i32;
    let mut esc = false;
    for c in s.chars() {
        if let Some(qc) = q {
            cur.push(c);
            if esc {
                esc = false;
            } else if c == '\\' && qc == '"' {
                esc = true;
            } else if c == qc {
                q = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                q = Some(c);
                cur.push(c);
            }
            '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ']' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn inner(raw: &str, open: char, close: char) -> Option<&str> {
    let s = strip_comment(raw.lines().next().unwrap_or("")).trim();
    s.strip_prefix(open)?.strip_suffix(close)
}

/// Parse a list: flow `[a, b]` or block (`- a` continuation lines).
pub fn list(raw: &str) -> Vec<String> {
    if let Some(i) = inner(raw, '[', ']') {
        return split_top(i).iter().map(|x| unquote(x)).collect();
    }
    raw.lines()
        .filter_map(|l| l.trim_start().strip_prefix("- "))
        .map(|l| unquote(strip_comment(l)))
        .collect()
}

pub fn fmt_list(items: &[String]) -> String {
    format!(
        "[{}]",
        items.iter().map(|s| quote_item(s)).collect::<Vec<_>>().join(", ")
    )
}

/// Parse a flow map `{ a: x, b: "y" }` into ordered pairs.
pub fn flow_map(raw: &str) -> Vec<(String, String)> {
    let Some(i) = inner(raw, '{', '}') else {
        return vec![];
    };
    split_top(i)
        .iter()
        .filter_map(|p| {
            let (k, v) = p.split_once(':')?;
            Some((unquote(k), unquote(v)))
        })
        .collect()
}

pub fn fmt_flow_map(pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return "{}".into();
    }
    format!(
        "{{ {} }}",
        pairs
            .iter()
            .map(|(k, v)| format!("{k}: {}", quote_item(v)))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_and_comments() {
        assert_eq!(
            scalar("none                                   # none | kept"),
            "none"
        );
        assert_eq!(scalar("\"a # b\""), "a # b");
        assert_eq!(scalar("'it''s'"), "it's");
        assert_eq!(quote("Weekly sync с Альфой"), "Weekly sync с Альфой");
        assert_eq!(quote("a: b"), "\"a: b\"");
        assert_eq!(scalar(&quote("x \"y\" \\ z: w")), "x \"y\" \\ z: w");
    }

    #[test]
    fn lists_and_maps() {
        assert_eq!(list("[ru, en]"), vec!["ru", "en"]);
        assert_eq!(list("[\"a, b\", c]"), vec!["a, b", "c"]);
        assert_eq!(list("\n  - x\n  - y"), vec!["x", "y"]);
        assert_eq!(fmt_list(&["a, b".into(), "c".into()]), "[\"a, b\", c]");
        let m = flow_map("{ me: \"MacBook Pro Microphone\", system: Zoom }");
        assert_eq!(m[0], ("me".into(), "MacBook Pro Microphone".into()));
        assert_eq!(fmt_flow_map(&m), "{ me: MacBook Pro Microphone, system: Zoom }");
    }

    #[test]
    fn split_keeps_unknown_and_continuations() {
        let md = "---\nlowk: 1\n# note\nx:\n  - a\n  - b\ny: z\n---\nbody\n";
        let (fm, body) = split(md);
        let fm = fm.unwrap();
        assert_eq!(body, "body\n");
        assert_eq!(fm.render(), md.replace("---\nbody\n", "---\n"));
        assert_eq!(list(fm.get("x").unwrap()), vec!["a", "b"]);
    }

    #[test]
    fn no_frontmatter() {
        assert!(split("# hi\n").0.is_none());
    }
}
