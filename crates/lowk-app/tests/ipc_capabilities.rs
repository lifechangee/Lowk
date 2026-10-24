//! Every IPC command must be declared in build.rs and allowed for at least one window,
//! otherwise the webview's call is refused silently in the real app (mock-IPC UI tests can't see it).
use std::collections::BTreeSet;
use std::path::Path;

/// Commands only the backend calls (never a webview).
const BACKEND_ONLY: &[&str] = &["overlay_show", "overlay_hide"];

fn quoted(s: &str) -> BTreeSet<String> {
    s.split('"')
        .skip(1)
        .step_by(2)
        .filter(|w| {
            !w.is_empty()
                && w.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn every_command_is_declared_and_allowed_for_a_window() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap();
    let registered: BTreeSet<String> = read("src/commands/mod.rs")
        .split("crate::commands::")
        .skip(1)
        .filter_map(|s| {
            s.split(|c: char| c == ',' || c.is_whitespace() || c == ']')
                .next()
        })
        .filter_map(|s| s.rsplit("::").next().map(str::to_string))
        .collect();
    assert!(registered.len() > 50, "parsed {} commands", registered.len());
    let declared = quoted(&read("build.rs"));
    let mut allowed = BTreeSet::new();
    for e in std::fs::read_dir(dir.join("capabilities")).unwrap() {
        let s = std::fs::read_to_string(e.unwrap().path()).unwrap();
        for w in s.split('"').filter_map(|w| w.strip_prefix("allow-")) {
            allowed.insert(w.replace('-', "_"));
        }
    }
    let undeclared: Vec<_> = registered.iter().filter(|c| !declared.contains(*c)).collect();
    assert!(undeclared.is_empty(), "missing in build.rs: {undeclared:?}");
    let unallowed: Vec<_> = registered
        .iter()
        .filter(|c| !allowed.contains(*c) && !BACKEND_ONLY.contains(&c.as_str()))
        .collect();
    assert!(
        unallowed.is_empty(),
        "not allowed for any window (capabilities/*.json): {unallowed:?}"
    );
}
