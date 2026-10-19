//! Key code table: W3C `KeyboardEvent.code` name <-> macOS virtual keycode <-> Windows VK.
//! Layout independent (physical ANSI positions), identical to the UI recorder.
use lowk_types::{Mods, MouseBtn, Os, Trigger};

/// (W3C code, macOS kVK, Windows VK)
pub(crate) const KEYS: &[(&str, u16, u16)] = &[
    ("KeyA", 0, 0x41),
    ("KeyS", 1, 0x53),
    ("KeyD", 2, 0x44),
    ("KeyF", 3, 0x46),
    ("KeyH", 4, 0x48),
    ("KeyG", 5, 0x47),
    ("KeyZ", 6, 0x5A),
    ("KeyX", 7, 0x58),
    ("KeyC", 8, 0x43),
    ("KeyV", 9, 0x56),
    ("KeyB", 11, 0x42),
    ("KeyQ", 12, 0x51),
    ("KeyW", 13, 0x57),
    ("KeyE", 14, 0x45),
    ("KeyR", 15, 0x52),
    ("KeyY", 16, 0x59),
    ("KeyT", 17, 0x54),
    ("Digit1", 18, 0x31),
    ("Digit2", 19, 0x32),
    ("Digit3", 20, 0x33),
    ("Digit4", 21, 0x34),
    ("Digit6", 22, 0x36),
    ("Digit5", 23, 0x35),
    ("Equal", 24, 0xBB),
    ("Digit9", 25, 0x39),
    ("Digit7", 26, 0x37),
    ("Minus", 27, 0xBD),
    ("Digit8", 28, 0x38),
    ("Digit0", 29, 0x30),
    ("BracketRight", 30, 0xDD),
    ("KeyO", 31, 0x4F),
    ("KeyU", 32, 0x55),
    ("BracketLeft", 33, 0xDB),
    ("KeyI", 34, 0x49),
    ("KeyP", 35, 0x50),
    ("Enter", 36, 0x0D),
    ("KeyL", 37, 0x4C),
    ("KeyJ", 38, 0x4A),
    ("Quote", 39, 0xDE),
    ("KeyK", 40, 0x4B),
    ("Semicolon", 41, 0xBA),
    ("Backslash", 42, 0xDC),
    ("Comma", 43, 0xBC),
    ("Slash", 44, 0xBF),
    ("KeyN", 45, 0x4E),
    ("KeyM", 46, 0x4D),
    ("Period", 47, 0xBE),
    ("Tab", 48, 0x09),
    ("Space", 49, 0x20),
    ("Backquote", 50, 0xC0),
    ("Backspace", 51, 0x08),
    ("Escape", 53, 0x1B),
    ("F17", 64, 0x80),
    ("F18", 79, 0x81),
    ("F19", 80, 0x82),
    ("F20", 90, 0x83),
    ("F5", 96, 0x74),
    ("F6", 97, 0x75),
    ("F7", 98, 0x76),
    ("F3", 99, 0x72),
    ("F8", 100, 0x77),
    ("F9", 101, 0x78),
    ("F11", 103, 0x7A),
    ("F13", 105, 0x7C),
    ("F16", 106, 0x7F),
    ("F14", 107, 0x7D),
    ("F10", 109, 0x79),
    ("F12", 111, 0x7B),
    ("F15", 113, 0x7E),
    ("Home", 115, 0x24),
    ("PageUp", 116, 0x21),
    ("Delete", 117, 0x2E),
    ("F4", 118, 0x73),
    ("End", 119, 0x23),
    ("F2", 120, 0x71),
    ("PageDown", 121, 0x22),
    ("F1", 122, 0x70),
    ("ArrowLeft", 123, 0x25),
    ("ArrowRight", 124, 0x27),
    ("ArrowDown", 125, 0x28),
    ("ArrowUp", 126, 0x26),
];

/// True when `code` is a name we can register on both platforms.
pub fn is_known_code(code: &str) -> bool {
    KEYS.iter().any(|k| k.0 == code)
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(crate) fn from_mac_vk(vk: u16) -> &'static str {
    KEYS.iter().find(|k| k.1 == vk).map_or("Unknown", |k| k.0)
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn from_win_vk(vk: u16) -> &'static str {
    KEYS.iter().find(|k| k.2 == vk).map_or("Unknown", |k| k.0)
}

/// Human label for UI/logs, e.g. `Cmd+Shift+S`, `Option`, `Mouse Back`.
pub fn describe(os: Os, t: &Trigger) -> String {
    let mac = os == Os::Macos;
    let mods = |m: &Mods| {
        let mut v: Vec<String> = Vec::new();
        if m.ctrl {
            v.push("Ctrl".into());
        }
        if m.alt {
            v.push(if mac { "Option" } else { "Alt" }.into());
        }
        if m.shift {
            v.push("Shift".into());
        }
        if m.meta {
            v.push(if mac { "Cmd" } else { "Win" }.into());
        }
        v
    };
    let key = |k: &str| {
        k.strip_prefix("Key")
            .or_else(|| k.strip_prefix("Digit"))
            .unwrap_or(k)
            .to_owned()
    };
    match t {
        Trigger::Combo { mods: m, key: k } => {
            let mut v = mods(m);
            v.push(key(&k.0));
            v.join("+")
        }
        Trigger::ModifierOnly { mods: m } => mods(m).join("+"),
        Trigger::Mouse { button, mods: m } => {
            let mut v = mods(m);
            v.push(match button {
                MouseBtn::Middle => "Middle".to_owned(),
                MouseBtn::Back => "Mouse Back".to_owned(),
                MouseBtn::Forward => "Mouse Forward".to_owned(),
                MouseBtn::Other(n) => format!("Mouse {n}"),
            });
            v.join("+")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::KeyCode;

    #[test]
    fn table_has_no_duplicates() {
        for (i, a) in KEYS.iter().enumerate() {
            for b in &KEYS[i + 1..] {
                assert_ne!(a.0, b.0);
                assert_ne!(a.1, b.1, "{} {}", a.0, b.0);
                assert_ne!(a.2, b.2, "{} {}", a.0, b.0);
            }
        }
    }

    #[test]
    fn lookup_and_describe() {
        assert_eq!(from_mac_vk(36), "Enter");
        assert_eq!(from_win_vk(0xDC), "Backslash");
        assert_eq!(from_mac_vk(999), "Unknown");
        let t = Trigger::Combo {
            mods: Mods::META_SHIFT,
            key: KeyCode::new("KeyS"),
        };
        assert_eq!(describe(Os::Macos, &t), "Shift+Cmd+S");
        assert_eq!(describe(Os::Windows, &t), "Shift+Win+S");
        assert_eq!(
            describe(Os::Macos, &Trigger::ModifierOnly { mods: Mods::ALT }),
            "Option"
        );
    }
}
