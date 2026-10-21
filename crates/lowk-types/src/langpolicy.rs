//! Call languages and the script policy that follows from them.
//!
//! Std only on purpose: the STT worker (`workers/lowk-stt-worker`, its own cargo workspace) includes
//! this very file with `#[path]`, so the policy cannot drift between the app and the worker.
//!
//! A call has a set of languages (BCP-47 primary tags: `ru`, `en`, `pl`, ...). The set drives
//! * the decoder hint (one language -> its prompt tag, several -> the model's auto slot),
//! * the allowed-letters guard: a letter is *foreign* when no selected language writes it
//!   (`ł` is fine with `pl` selected and garbage with `ru+en`; Cyrillic is garbage with `en` only),
//! * the Whisper correction rejection rule (same guard),
//! * the call frontmatter `languages`.

/// A language the local engine (Nemotron 3.5 ASR Streaming) can decode: the codes of the model's own
/// prompt table (`<xx-XX>` tokens in the GGUF). `code` is the primary tag stored in settings and
/// frontmatter, `tag` the decoder prompt, `latin` the non-ASCII Latin letters of its alphabet.
pub struct Lang {
    pub code: &'static str,
    pub tag: &'static str,
    pub script: Script,
    /// Lowercase non-ASCII letters this language writes (Latin scripts); empty otherwise.
    pub latin: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Latin,
    Cyrillic,
    Greek,
    Hebrew,
    Arabic,
    Devanagari,
    Thai,
    Kana,
    Hangul,
    Han,
}

const VI: &str = "àáạảãăằắặẳẵâầấậẩẫđèéẹẻẽêềếệểễìíịỉĩòóọỏõôồốộổỗơờớợởỡùúụủũưừứựửữỳýỵỷỹ";

/// Order = the order shown in pickers after the user's own languages.
pub const LANGS: &[Lang] = &[
    l("ru", "ru-RU", Script::Cyrillic, ""),
    l("en", "en-US", Script::Latin, ""),
    l("uk", "uk-UA", Script::Cyrillic, ""),
    l("de", "de-DE", Script::Latin, "äöüß"),
    l("fr", "fr-FR", Script::Latin, "àâæçéèêëîïôœùûüÿ"),
    l("es", "es-ES", Script::Latin, "áéíóúüñ"),
    l("it", "it-IT", Script::Latin, "àèéìíîòóùú"),
    l("pt", "pt-BR", Script::Latin, "áâãàçéêíóôõúü"),
    l("nl", "nl-NL", Script::Latin, "éèêëïöüáäíóú"),
    l("pl", "pl-PL", Script::Latin, "ąćęłńóśźż"),
    l("cs", "cs-CZ", Script::Latin, "áčďéěíňóřšťúůýž"),
    l("sk", "sk-SK", Script::Latin, "áäčďéíĺľňóôŕšťúýž"),
    l("sl", "sl-SL", Script::Latin, "čšž"),
    l("hr", "hr-HR", Script::Latin, "čćđšžǆǉǌ"),
    l("sv", "sv-SE", Script::Latin, "åäöé"),
    l("da", "da-DK", Script::Latin, "æøåé"),
    l("no", "nb-NO", Script::Latin, "æøåé"),
    l("fi", "fi-FI", Script::Latin, "äöåšž"),
    l("hu", "hu-HU", Script::Latin, "áéíóöőúüű"),
    l("ro", "ro-RO", Script::Latin, "ăâîșțşţ"),
    l("tr", "tr-TR", Script::Latin, "çğıİöşüâîû"),
    l("lt", "lt-LT", Script::Latin, "ąčęėįšųūž"),
    l("lv", "lv-LV", Script::Latin, "āčēģīķļņšūž"),
    l("et", "et-EE", Script::Latin, "äõöüšž"),
    l("vi", "vi-VN", Script::Latin, VI),
    l("bg", "bg-BG", Script::Cyrillic, ""),
    l("el", "el-GR", Script::Greek, ""),
    l("he", "he-IL", Script::Hebrew, ""),
    l("ar", "ar-AR", Script::Arabic, ""),
    l("hi", "hi-IN", Script::Devanagari, ""),
    l("th", "th-TH", Script::Thai, ""),
    l("ja", "ja-JP", Script::Kana, ""),
    l("ko", "ko-KR", Script::Hangul, ""),
    l("zh", "zh-CN", Script::Han, ""),
];

const fn l(code: &'static str, tag: &'static str, script: Script, latin: &'static str) -> Lang {
    Lang {
        code,
        tag,
        script,
        latin,
    }
}

/// Primary tag of a language tag: `ru-RU` -> `ru`, `nb` / `nn` -> `no`, `en_US.UTF-8` -> `en`.
pub fn primary(tag: &str) -> String {
    let p = tag
        .split(['-', '_', '.', '@'])
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    match p.as_str() {
        "nb" | "nn" => "no".into(),
        _ => p,
    }
}

pub fn lang(code: &str) -> Option<&'static Lang> {
    LANGS.iter().find(|x| x.code == code)
}

/// The codes the local engine supports.
pub fn engine_codes() -> Vec<String> {
    LANGS.iter().map(|x| x.code.to_owned()).collect()
}

/// Decoder prompt of a language (`ru` -> `ru-RU`).
pub fn engine_tag(code: &str) -> Option<&'static str> {
    lang(code).map(|x| x.tag)
}

/// Decoder language argument for a language set: the prompt of a single known language, else `auto`.
pub fn decoder_lang(langs: &[String]) -> String {
    match langs {
        [one] => engine_tag(one).map_or_else(|| "auto".into(), str::to_owned),
        _ => "auto".into(),
    }
}

/// Lowercase, primary subtag, de-duplicated, order kept; unknown junk (empty) dropped.
pub fn normalize(codes: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in codes {
        let p = primary(c);
        if !p.is_empty() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// What a call really uses: the requested set (or the default when none) limited to what the engine
/// supports (`supported` empty = no limit). An empty result falls back to the default, then to the
/// whole supported set's first entry, so a call never starts with no language.
pub fn resolve(requested: Option<&[String]>, default: &[String], supported: &[String]) -> Vec<String> {
    let keep = |v: &[String]| -> Vec<String> {
        normalize(v)
            .into_iter()
            .filter(|c| supported.is_empty() || supported.iter().any(|s| s == c))
            .collect()
    };
    let mut r = requested.map(keep).unwrap_or_default();
    if r.is_empty() {
        r = keep(default);
    }
    if r.is_empty() {
        r = ["en".to_owned()]
            .into_iter()
            .filter(|c| supported.is_empty() || supported.contains(c))
            .chain(supported.first().cloned())
            .take(1)
            .collect();
    }
    r
}

/// Default for a fresh install: the system language plus English (`ru_RU` -> ru+en, `en_US` -> en,
/// `de` -> de+en). A system language the engine cannot decode gives English only.
pub fn system_default(locale: &str, supported: &[String]) -> Vec<String> {
    let sys = primary(locale);
    let ok = |c: &str| supported.is_empty() || supported.iter().any(|s| s == c);
    let mut v = Vec::new();
    if !sys.is_empty() && ok(&sys) {
        v.push(sys.clone());
    }
    if sys != "en" && ok("en") {
        v.push("en".into());
    }
    if v.is_empty() {
        v.push("en".into());
    }
    v
}

fn has_script(allowed: &[String], s: Script) -> bool {
    allowed.iter().any(|a| lang(a).is_some_and(|l| l.script == s))
}

fn has_code(allowed: &[String], codes: &[&str]) -> bool {
    allowed.iter().any(|a| codes.contains(&a.as_str()))
}

fn latin_ok(c: char, allowed: &[String]) -> bool {
    let lower = c.to_lowercase().next().unwrap_or(c);
    allowed.iter().any(|a| {
        lang(a).is_some_and(|l| {
            l.latin.contains(c)
                || l.latin.contains(lower)
                || (l.code == "vi" && ('\u{1E00}'..='\u{1EFF}').contains(&c))
        })
    })
}

/// Is `c` a letter the call's languages (`allowed`) cannot contain? ASCII is always fine (names, terms,
/// code-switching into English words). Cyrillic needs a Cyrillic language (and і ї є ґ ў need `uk`/`be`),
/// accented Latin needs a language that writes that letter, Greek/Hebrew/Arabic/CJK/... their language.
/// An empty list allows everything.
pub fn is_foreign_letter(c: char, allowed: &[String]) -> bool {
    if allowed.is_empty() || !c.is_alphabetic() || c.is_ascii() {
        return false;
    }
    match c {
        'і' | 'ї' | 'є' | 'ґ' | 'І' | 'Ї' | 'Є' | 'Ґ' | 'ў' | 'Ў' => {
            !has_code(allowed, &["uk", "be"])
        }
        '\u{0400}'..='\u{04FF}' => !has_script(allowed, Script::Cyrillic),
        '\u{00C0}'..='\u{024F}' | '\u{1E00}'..='\u{1EFF}' => !latin_ok(c, allowed),
        '\u{0370}'..='\u{03FF}' => !has_script(allowed, Script::Greek),
        '\u{0590}'..='\u{05FF}' => !has_script(allowed, Script::Hebrew),
        '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' => !has_script(allowed, Script::Arabic),
        '\u{0900}'..='\u{097F}' => !has_script(allowed, Script::Devanagari),
        '\u{0E00}'..='\u{0E7F}' => !has_script(allowed, Script::Thai),
        '\u{3040}'..='\u{30FF}' => !has_code(allowed, &["ja"]),
        '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' => !has_script(allowed, Script::Hangul),
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' => !has_code(allowed, &["zh", "ja"]),
        _ => true,
    }
}

/// Number of letters outside the call's languages.
pub fn foreign_letter_count(s: &str, allowed: &[String]) -> usize {
    s.chars().filter(|c| is_foreign_letter(*c, allowed)).count()
}

/// Last resort when a re-decode still has foreign letters: drops every word that contains one
/// (a half-garbled word is worse than a gap) and tidies the punctuation left behind.
pub fn strip_foreign(s: &str, allowed: &[String]) -> String {
    if foreign_letter_count(s, allowed) == 0 {
        return s.to_owned();
    }
    s.split_whitespace()
        .filter(|w| !w.chars().any(|c| is_foreign_letter(c, allowed)))
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Prompts for the re-decode of a drifted utterance, in the user's order, at most `max`.
pub fn rescue_tags(allowed: &[String], max: usize) -> Vec<&'static str> {
    allowed.iter().filter_map(|a| engine_tag(a)).take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn table_is_consistent() {
        for (i, a) in LANGS.iter().enumerate() {
            assert!(
                a.tag
                    .starts_with(&format!("{}-", if a.code == "no" { "nb" } else { a.code })),
                "{}",
                a.code
            );
            assert!(LANGS[i + 1..].iter().all(|b| b.code != a.code));
            assert!(a.latin.is_empty() || a.script == Script::Latin);
        }
    }

    #[test]
    fn ru_en_policy() {
        let a = s(&["ru", "en"]);
        assert_eq!(foreign_letter_count("Подведём итоги от Sales, ёжик", &a), 0);
        assert_eq!(foreign_letter_count("Підбегум", &a), 1);
    }

    #[test]
    fn ru_en_rejects_polish_ukrainian_arabic_cjk() {
        let a = s(&["ru", "en"]);
        assert_eq!(foreign_letter_count("Bułżą rauncie? Ебета.", &a), 3);
        assert_eq!(foreign_letter_count("Okay 可 我 أ", &a), 3);
        assert_eq!(foreign_letter_count("З фідбиком", &a), 1);
        assert_eq!(foreign_letter_count("café", &a), 1);
        assert_eq!(
            strip_foreign("Давай Bułżą rauncie? Ебета.", &a),
            "Давай rauncie? Ебета."
        );
    }

    #[test]
    fn polish_selected_allows_polish_letters_only() {
        let a = s(&["pl", "en"]);
        assert_eq!(foreign_letter_count("Bułżą źdźbło Łódź ęń ć", &a), 0);
        assert_eq!(foreign_letter_count("Schöne Grüße", &a), 3); // ö ü ß
        assert_eq!(foreign_letter_count("Привет", &a), 6);
        assert_eq!(strip_foreign("to jest Bułżą über", &a), "to jest Bułżą");
    }

    #[test]
    fn german_french_spanish_turkish() {
        assert_eq!(foreign_letter_count("Schöne Grüße ÄÖÜ", &s(&["de"])), 0);
        assert_eq!(foreign_letter_count("Schöne ł", &s(&["de"])), 1);
        assert_eq!(
            foreign_letter_count("Où est le garçon, Œuvre", &s(&["fr", "en"])),
            0
        );
        assert_eq!(foreign_letter_count("¿Cómo está, señor? Ñandú", &s(&["es"])), 0);
        assert_eq!(foreign_letter_count("İstanbul ığş", &s(&["tr"])), 0);
        assert_eq!(foreign_letter_count("ığ", &s(&["de", "en"])), 2);
    }

    #[test]
    fn cyrillic_needs_a_cyrillic_language() {
        assert_eq!(foreign_letter_count("Привет, hello", &s(&["en"])), 6);
        assert_eq!(foreign_letter_count("Привет, hello", &s(&["en", "ru"])), 0);
        assert_eq!(foreign_letter_count("Привіт", &s(&["uk", "en"])), 0);
        assert_eq!(foreign_letter_count("Привіт", &s(&["ru"])), 1);
        assert_eq!(foreign_letter_count("Здравей", &s(&["bg"])), 0);
    }

    #[test]
    fn other_scripts_need_their_language() {
        assert_eq!(foreign_letter_count("你好 こんにちは 안녕", &s(&["en"])), 9);
        assert_eq!(foreign_letter_count("你好", &s(&["zh"])), 0);
        assert_eq!(foreign_letter_count("你好 こんにちは", &s(&["ja"])), 0);
        assert_eq!(foreign_letter_count("你好 こんにちは", &s(&["zh"])), 5);
        assert_eq!(foreign_letter_count("안녕", &s(&["ko", "en"])), 0);
        assert_eq!(foreign_letter_count("مرحبا שלום", &s(&["ar"])), 4);
        assert_eq!(foreign_letter_count("Γειά", &s(&["el"])), 0);
        assert_eq!(foreign_letter_count("Việt Nam ạ", &s(&["vi"])), 0);
    }

    #[test]
    fn empty_set_allows_everything_and_ascii_is_always_fine() {
        assert_eq!(foreign_letter_count("anything ł 可 Ебета", &[]), 0);
        assert_eq!(foreign_letter_count("plain ASCII, Sales, API-v2", &s(&["ru"])), 0);
        assert_eq!(strip_foreign("x 可 y", &s(&["en"])), "x y");
    }

    #[test]
    fn decoder_lang_single_known_language_else_auto() {
        assert_eq!(decoder_lang(&s(&["ru"])), "ru-RU");
        assert_eq!(decoder_lang(&s(&["en"])), "en-US");
        assert_eq!(decoder_lang(&s(&["pl"])), "pl-PL");
        assert_eq!(decoder_lang(&s(&["ru", "en"])), "auto");
        assert_eq!(decoder_lang(&s(&["xx"])), "auto");
        assert_eq!(
            rescue_tags(&s(&["ru", "xx", "en", "pl"]), 2),
            vec!["ru-RU", "en-US"]
        );
    }

    #[test]
    fn resolve_limits_to_engine_and_never_returns_empty() {
        let sup = s(&["ru", "en", "pl"]);
        assert_eq!(
            resolve(Some(&s(&["pl", "RU-ru", "pl", "xx"])), &s(&["en"]), &sup),
            s(&["pl", "ru"])
        );
        assert_eq!(resolve(None, &s(&["ru", "en"]), &sup), s(&["ru", "en"]));
        assert_eq!(resolve(Some(&s(&["xx"])), &s(&["ru"]), &sup), s(&["ru"]));
        assert_eq!(resolve(Some(&[]), &s(&["xx"]), &sup), s(&["en"]));
        assert_eq!(resolve(Some(&s(&["xx"])), &s(&["yy"]), &[]), s(&["xx"]));
    }

    #[test]
    fn system_default_is_system_plus_english() {
        let sup = engine_codes();
        assert_eq!(system_default("ru_RU.UTF-8", &sup), s(&["ru", "en"]));
        assert_eq!(system_default("en-US", &sup), s(&["en"]));
        assert_eq!(system_default("de", &sup), s(&["de", "en"]));
        assert_eq!(system_default("sw-KE", &sup), s(&["en"]));
        assert_eq!(system_default("nb-NO", &sup), s(&["no", "en"]));
    }
}
