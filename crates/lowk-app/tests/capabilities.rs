//! Regression: «Добавить документ» showed «неизвестная ошибка» because `doc_find_duplicates` was
//! registered but not allowed in the main window's capability, so Tauri rejected the call with a
//! plain string. Every command must be allowed somewhere, and every command the main UI calls must
//! be allowed for the main window.
use std::collections::BTreeSet;

fn perms(file: &str) -> BTreeSet<String> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("capabilities")
        .join(file);
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    v["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x.as_str().map(String::from))
        .collect()
}

fn commands() -> Vec<String> {
    let b =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs")).unwrap();
    let a = b.find("const COMMANDS").unwrap();
    b[a..b[a..].find("];").unwrap() + a]
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"')?.strip_suffix("\",").map(String::from))
        .collect()
}

fn allow(c: &str) -> String {
    format!("allow-{}", c.replace('_', "-"))
}

#[test]
fn every_command_is_allowed_by_some_capability() {
    let all: BTreeSet<String> = perms("main.json")
        .union(&perms("overlay.json"))
        .chain(perms("area.json").iter())
        .cloned()
        .collect();
    // driven from Rust only (hotkeys, tray); no webview ever invokes them
    const BACKEND_ONLY: &[&str] = &["overlay_show", "overlay_hide"];
    let missing: Vec<String> = commands()
        .into_iter()
        .filter(|c| !BACKEND_ONLY.contains(&c.as_str()) && !all.contains(&allow(c)))
        .collect();
    assert!(missing.is_empty(), "commands without a capability: {missing:?}");
}

#[test]
fn every_command_the_main_ui_calls_is_allowed_for_main() {
    let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/main");
    let mut src = String::new();
    let mut stack = vec![ui];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "ts" || x == "svelte") {
                src.push_str(&std::fs::read_to_string(p).unwrap());
            }
        }
    }
    let main = perms("main.json");
    let known = commands();
    let mut missing = vec![];
    for c in &known {
        let camel: String = {
            let mut s = String::new();
            let mut up = false;
            for ch in c.chars() {
                if ch == '_' {
                    up = true;
                } else if up {
                    s.extend(ch.to_uppercase());
                    up = false;
                } else {
                    s.push(ch);
                }
            }
            s
        };
        if src.contains(&format!("commands.{camel}(")) && !main.contains(&allow(c)) {
            missing.push(c.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "main UI calls commands main.json does not allow: {missing:?}"
    );
}

#[test]
fn the_area_window_may_only_report_its_selection() {
    let p = perms("area.json");
    let want: BTreeSet<String> = ["core:event:default", "allow-area-confirm", "allow-area-cancel"]
        .map(String::from)
        .into();
    assert_eq!(
        p, want,
        "the selection page gets events and its two commands, nothing else"
    );
    for other in ["main.json", "overlay.json"] {
        assert!(
            !perms(other).contains("allow-area-confirm"),
            "{other} must not confirm a selection"
        );
    }
}
