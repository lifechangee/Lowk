//! System locale. UI languages are `ru` and `en`; everything else falls back to `en`.

/// Raw system locale (BCP-47 such as `ru-RU`), `en-US` when unknown.
pub fn system() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string())
}

/// Maps a locale tag to a supported UI language code (`"ru"` or `"en"`).
pub fn resolve(tag: &str) -> &'static str {
    let primary = tag
        .split(['-', '_', '.'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if primary == "ru" {
        "ru"
    } else {
        "en"
    }
}

/// UI language derived from the system locale.
pub fn ui_lang() -> &'static str {
    resolve(&system())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping() {
        assert_eq!(resolve("ru-RU"), "ru");
        assert_eq!(resolve("ru_RU.UTF-8"), "ru");
        assert_eq!(resolve("RU"), "ru");
        assert_eq!(resolve("en-GB"), "en");
        assert_eq!(resolve("de-DE"), "en");
        assert_eq!(resolve(""), "en");
        assert_eq!(resolve("uk-UA"), "en");
    }
}
