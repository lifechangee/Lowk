//! Text hygiene shared by the STT, session and store crates (S1: «�» in saved calls).
//!
//! Every place that shortens or stores speech text goes through here, so a cut can never land
//! inside a UTF-8 sequence or a grapheme cluster (emoji + ZWJ, combining marks), and a
//! replacement character from a half-decoded token never reaches a call file.
use unicode_segmentation::UnicodeSegmentation;

/// U+FFFD: what lossy UTF-8 decoding leaves where bytes were cut or invalid.
pub const REPLACEMENT: char = '\u{FFFD}';

/// True when `s` contains a replacement character.
pub fn has_replacement(s: &str) -> bool {
    s.contains(REPLACEMENT)
}

/// Longest prefix of `s` with at most `max` grapheme clusters (never splits a character).
pub fn truncate_graphemes(s: &str, max: usize) -> &str {
    match s.grapheme_indices(true).nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Longest prefix of `s` that is at most `max_bytes` long and ends on a grapheme boundary.
/// (The byte budget form: for protocols/limits that count bytes.)
pub fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = 0;
    for (i, g) in s.grapheme_indices(true) {
        if i + g.len() > max_bytes {
            break;
        }
        end = i + g.len();
    }
    &s[..end]
}

/// `truncate_graphemes` plus an ellipsis when something was cut.
pub fn ellipsize(s: &str, max: usize) -> String {
    let t = truncate_graphemes(s, max);
    if t.len() == s.len() {
        s.to_owned()
    } else {
        format!("{}…", t.trim_end())
    }
}

/// Speech text as the engine produced it, made safe to store and show: replacement characters
/// and control characters are dropped (a char whose bytes were split by a token boundary cannot
/// be recovered, but the word stays readable), runs of whitespace collapse to one space.
pub fn clean_speech(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c == REPLACEMENT || (c.is_control() && !c.is_whitespace()) {
            continue;
        }
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(c);
    }
    out
}

// S3 (language drift): the script policy lives in `langpolicy` (shared with the STT worker).
pub use crate::langpolicy::{foreign_letter_count, is_foreign_letter, strip_foreign};

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic generator over the alphabets that bit us: Cyrillic, Latin, emoji,
    /// ZWJ sequences, combining marks, flags.
    fn corpus(seed: u64, n: usize) -> String {
        const POOL: &[&str] = &[
            "а",
            "б",
            "ё",
            "к",
            "я",
            "Ю",
            "щ",
            "ы",
            "ь",
            "A",
            "z",
            "5",
            " ",
            ",",
            ".",
            "👍",
            "😀",
            "👨\u{200D}👩\u{200D}👧",
            "🇷🇺",
            "e\u{301}",
            "й",
            "и\u{306}",
            "\u{FFFD}",
        ];
        let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                POOL[(x % POOL.len() as u64) as usize]
            })
            .collect()
    }

    #[test]
    fn property_cuts_never_split_a_character_or_cluster() {
        for seed in 0..400 {
            let s = corpus(seed, (seed % 40) as usize + 1);
            for max in 0..(s.len() + 2) {
                // bytes: valid UTF-8 by construction of &str; also within budget, and on a cluster edge
                let t = truncate_bytes(&s, max);
                assert!(t.len() <= max.max(if s.len() <= max { s.len() } else { 0 }));
                assert!(s.starts_with(t));
                assert!(
                    t.len() == s.len() || s.grapheme_indices(true).any(|(i, _)| i == t.len()),
                    "cut inside a grapheme: {s:?} @ {max} -> {t:?}"
                );
                // graphemes: exactly min(max, count) clusters
                let g = truncate_graphemes(&s, max);
                assert_eq!(g.graphemes(true).count(), max.min(s.graphemes(true).count()));
                assert!(s.starts_with(g));
            }
        }
    }

    #[test]
    fn cyrillic_and_emoji_examples() {
        let s = "Расскажи коротко 👨\u{200D}👩\u{200D}👧 ок";
        assert_eq!(truncate_bytes(s, 11), "Расск"); // 5 letters = 10 bytes, the 6th would cross 11
        assert_eq!(truncate_bytes(s, 1), "");
        assert!(!has_replacement(truncate_bytes(s, 29)));
        let family = "👨\u{200D}👩\u{200D}👧";
        assert_eq!(
            truncate_graphemes("а👨\u{200D}👩\u{200D}👧б", 2),
            format!("а{family}")
        );
        assert_eq!(ellipsize("Расскажи коротко", 8), "Расскажи…");
        assert_eq!(ellipsize("ок", 8), "ок");
    }

    #[test]
    fn clean_speech_drops_replacement_and_controls() {
        assert_eq!(
            clean_speech("А, о\u{FFFD}кей.  Ну\t расскажи\u{0007}"),
            "А, окей. Ну расскажи"
        );
        assert_eq!(clean_speech("  \u{FFFD} "), "");
        for seed in 0..200 {
            let c = clean_speech(&corpus(seed, 30));
            assert!(!has_replacement(&c));
            assert!(!c.starts_with(' ') && !c.ends_with(' ') && !c.contains("  "));
        }
    }

    #[test]
    fn s3_foreign_letters_for_ru_en() {
        let a: Vec<String> = vec!["ru".into(), "en".into()];
        assert_eq!(
            foreign_letter_count("Bułżą rauncie? Ебета. Let's go, ёжик", &a),
            3
        );
        assert_eq!(foreign_letter_count("Підбегум 可 أ č", &a), 4);
        assert_eq!(foreign_letter_count("Schöne Grüße", &["de".into()]), 0);
        assert_eq!(foreign_letter_count("anything ł", &[]), 0);
        assert_eq!(
            strip_foreign("Давай Bułżą rauncie? Ебета.", &a),
            "Давай rauncie? Ебета."
        );
        assert_eq!(strip_foreign("Okay 可 我 now", &a), "Okay now");
    }
}
