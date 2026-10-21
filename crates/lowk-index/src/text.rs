//! Tokenisation, RU+EN stemming and token estimates.
//!
//! FTS5 has no Russian stemmer, so text is stemmed here (Snowball) and the stems are what the
//! FTS table holds. Tokens are cut at script boundaries (`Redis7` -> `redis`, `7`), so mixed
//! RU/EN speech ("что значит EBITDA") matches in either script.
use rust_stemmers::{Algorithm, Stemmer};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Script {
    Cyr,
    Lat,
    Digit,
    Other,
}

fn script(c: char) -> Option<Script> {
    if c.is_ascii_digit() || (c.is_numeric() && !c.is_alphabetic()) {
        Some(Script::Digit)
    } else if !c.is_alphabetic() {
        None
    } else if ('\u{0400}'..='\u{052F}').contains(&c) {
        Some(Script::Cyr)
    } else if c.is_ascii_alphabetic() || ('\u{00C0}'..='\u{024F}').contains(&c) {
        Some(Script::Lat)
    } else {
        Some(Script::Other)
    }
}

fn ru() -> &'static Stemmer {
    static S: OnceLock<Stemmer> = OnceLock::new();
    S.get_or_init(|| Stemmer::create(Algorithm::Russian))
}
fn en() -> &'static Stemmer {
    static S: OnceLock<Stemmer> = OnceLock::new();
    S.get_or_init(|| Stemmer::create(Algorithm::English))
}

/// One word of the input: byte range in the source, lower-cased stem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub start: usize,
    pub end: usize,
    pub stem: String,
}

fn stem_word(w: &str, s: Script) -> String {
    let lower = w.to_lowercase();
    match s {
        // е and ё are written interchangeably
        Script::Cyr => ru().stem(&lower.replace('ё', "е")).into_owned(),
        Script::Lat => en().stem(&lower).into_owned(),
        _ => lower,
    }
}

/// Splits `text` into stemmed words with their byte offsets.
pub fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let push = |out: &mut Vec<Word>, start: usize, end: usize, s: Script| {
        if end > start {
            out.push(Word {
                start,
                end,
                stem: stem_word(&text[start..end], s),
            });
        }
    };
    let mut cur: Option<(usize, Script)> = None;
    for (i, c) in text.char_indices() {
        let sc = script(c);
        match (cur, sc) {
            (Some((_, a)), Some(b)) if a == b => {}
            (Some((st, a)), Some(b)) => {
                push(&mut out, st, i, a);
                cur = Some((i, b));
            }
            (Some((st, a)), None) => {
                push(&mut out, st, i, a);
                cur = None;
            }
            (None, Some(b)) => cur = Some((i, b)),
            (None, None) => {}
        }
    }
    if let Some((st, a)) = cur {
        push(&mut out, st, text.len(), a);
    }
    out
}

/// Stemmed text for the FTS table.
pub fn stem_text(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for (i, x) in words(text).iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&x.stem);
    }
    s
}

const STOP: &[&str] = &[
    "the",
    "a",
    "an",
    "and",
    "or",
    "of",
    "to",
    "in",
    "on",
    "at",
    "is",
    "are",
    "was",
    "were",
    "be",
    "it",
    "this",
    "that",
    "for",
    "with",
    "as",
    "by",
    "do",
    "does",
    "did",
    "what",
    "how",
    "why",
    "when",
    "where",
    "which",
    "who",
    "i",
    "we",
    "you",
    "he",
    "she",
    "they",
    "my",
    "our",
    "your",
    "from",
    "about",
    "can",
    "will",
    "would",
    "should",
    "и",
    "в",
    "во",
    "не",
    "что",
    "он",
    "на",
    "я",
    "с",
    "со",
    "как",
    "а",
    "то",
    "все",
    "она",
    "так",
    "его",
    "но",
    "да",
    "ты",
    "к",
    "у",
    "же",
    "вы",
    "за",
    "бы",
    "по",
    "только",
    "ее",
    "мне",
    "было",
    "вот",
    "от",
    "меня",
    "еще",
    "нет",
    "о",
    "из",
    "ему",
    "теперь",
    "когда",
    "даже",
    "ну",
    "ли",
    "если",
    "уже",
    "или",
    "ни",
    "быть",
    "был",
    "для",
    "это",
    "этот",
    "эта",
    "эти",
    "чем",
    "где",
    "есть",
    "мы",
    "они",
    "там",
    "здесь",
    "какой",
    "какая",
    "какие",
    "такое",
    "значит",
    "про",
    "при",
    "над",
    "под",
    "без",
    "до",
    "ей",
    "их",
    "чтобы",
    "тоже",
    "ещё",
];

/// Stems of the content words of a query (stop words dropped unless nothing else is left),
/// deduplicated, order kept.
pub fn query_stems(text: &str) -> Vec<String> {
    let all = words(text);
    let mut keep: Vec<String> = vec![];
    let mut any: Vec<String> = vec![];
    for w in &all {
        let raw = text[w.start..w.end].to_lowercase();
        if !any.contains(&w.stem) {
            any.push(w.stem.clone());
        }
        if !STOP.contains(&raw.as_str()) && !keep.contains(&w.stem) {
            keep.push(w.stem.clone());
        }
    }
    if keep.is_empty() {
        any
    } else {
        keep
    }
}

/// Rough token count used for chunk sizes and budgets (Cyrillic costs about 2.5 chars per
/// token, Latin about 4).
pub fn estimate_tokens(text: &str) -> u32 {
    let mut cyr = 0f32;
    let mut other = 0f32;
    for c in text.chars() {
        if ('\u{0400}'..='\u{052F}').contains(&c) {
            cyr += 1.0;
        } else if !c.is_whitespace() {
            other += 1.0;
        } else {
            other += 0.3;
        }
    }
    ((cyr / 2.5 + other / 4.0).ceil() as u32).max(1)
}

/// UTF-16 offset of byte index `b` in `s`.
pub fn utf16_offset(s: &str, b: usize) -> u32 {
    s[..b].encode_utf16().count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_both_scripts_and_splits_on_boundaries() {
        let s = stem_text("Redis7 и звонки");
        let parts: Vec<_> = s.split(' ').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[1], "7");
        assert_eq!(stem_text("calls"), stem_text("call"));
        assert_eq!(stem_text("договоры"), stem_text("договор"));
        assert_eq!(stem_text("ёлка"), stem_text("елка"));
    }

    #[test]
    fn offsets_are_utf16() {
        let s = "😀 EBITDA";
        let w = words(s);
        assert_eq!(w.len(), 1);
        assert_eq!(utf16_offset(s, w[0].start), 3);
    }
}
