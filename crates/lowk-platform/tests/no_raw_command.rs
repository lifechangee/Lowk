//! Regression: a console window flashed on Windows because child processes were spawned with a raw
//! `Command::new`. Every spawn goes through `lowk_platform::process`; this scan keeps it that way.
use std::path::{Path, PathBuf};

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn no_raw_command_new_outside_the_process_helper() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let mut files = vec![];
    for d in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        rs_files(&d.path().join("src"), &mut files);
    }
    let mut bad = vec![];
    for f in files {
        if f.ends_with("lowk-platform/src/process.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        for (i, l) in text.lines().enumerate() {
            let t = l.trim_start();
            if t.starts_with("//") {
                continue;
            }
            if l.contains("Command::new(") || l.contains("tauri_plugin_shell") || l.contains(".sidecar(") {
                bad.push(format!("{}:{}: {}", f.display(), i + 1, t));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "use lowk_platform::process::command() instead of a raw Command::new:\n{}",
        bad.join("\n")
    );
}

#[test]
fn app_binary_is_a_gui_subsystem_app_in_every_profile() {
    let main = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../lowk-app/src/main.rs"))
        .unwrap();
    assert!(
        main.lines()
            .any(|l| l.trim() == "#![windows_subsystem = \"windows\"]"),
        "main.rs needs an unconditional #![windows_subsystem = \"windows\"] (debug builds included)"
    );
}
