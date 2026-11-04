//! Webview commands must be allowed for the window that invokes them.
use regex::Regex;
use std::{collections::BTreeMap, fs, path::Path};

fn scan(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, files);
        } else if matches!(path.extension().and_then(|s| s.to_str()), Some("ts" | "svelte")) {
            files.push(path);
        }
    }
}

#[test]
fn webview_commands_are_allowed_for_their_window() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bindings = fs::read_to_string(root.join("ui/src/lib/ipc/bindings.ts")).unwrap();
    let binding_re =
        Regex::new(r#"(?ms)^[ \t]*(\w+):.*?__TAURI_INVOKE(?:<[^()]+>)?\(\"([a-z0-9_]+)\""#).unwrap();
    let bindings: BTreeMap<_, _> = binding_re
        .captures_iter(&bindings)
        .map(|c| (c[1].to_owned(), c[2].to_owned()))
        .collect();
    assert!(bindings.len() > 50, "failed to parse generated bindings");
    let calls = Regex::new(r#"\bcommands\.(\w+)\s*\(|\binvoke(?:\s*<[^>]+>)?\s*\(\s*['\"]([a-z0-9_]+)['\"]"#)
        .unwrap();
    for window in ["overlay", "main"] {
        let capability: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(format!("crates/lowk-app/capabilities/{window}.json"))).unwrap(),
        )
        .unwrap();
        let permissions = capability["permissions"].as_array().unwrap();
        let mut files = Vec::new();
        scan(&root.join(format!("ui/src/{window}")), &mut files);
        let mut checked = 0;
        let mut missing = Vec::new();
        for file in files {
            let source = fs::read_to_string(&file).unwrap();
            for call in calls.captures_iter(&source) {
                let command = if let Some(name) = call.get(1) {
                    bindings
                        .get(name.as_str())
                        .unwrap_or_else(|| panic!("unknown binding {} in {}", name.as_str(), file.display()))
                        .as_str()
                } else {
                    call.get(2).unwrap().as_str()
                };
                checked += 1;
                let permission = format!("allow-{}", command.replace('_', "-"));
                if !permissions.iter().any(|p| p.as_str() == Some(&permission)) {
                    missing.push(format!("{}: {command} requires {permission}", file.display()));
                }
            }
        }
        assert!(checked > 0, "no {window} commands found");
        assert!(missing.is_empty(), "missing {window} permissions: {missing:#?}");
    }
}
