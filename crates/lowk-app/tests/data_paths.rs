//! The uninstall purge (`lowk-platform::purge`) is only complete if every on-disk location goes through the path
//! helpers in `lowk_platform::dirs`; these checks keep the two in step.
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn bundle_id_matches_tauri_conf() {
    let conf =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&conf).unwrap();
    // the WebView2 profile folder and the keychain service are named after it; the purge deletes both by this name
    assert_eq!(v["identifier"], lowk_platform::dirs::BUNDLE_ID);
    assert_eq!(lowk_platform::secrets::SERVICE, lowk_platform::dirs::BUNDLE_ID);
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            if !matches!(
                p.file_name().and_then(|n| n.to_str()),
                Some("target" | "node_modules" | "tests")
            ) {
                rs_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// No crate resolves its own per-user folder: a location the purge does not know about would survive uninstall.
#[test]
fn no_private_user_dirs_outside_the_path_helpers() {
    let mut files = vec![];
    rs_files(&root().join("crates"), &mut files);
    let allowed = ["lowk-platform/src/dirs.rs", "lowk-platform/src/purge.rs"];
    let needles = [
        "BaseDirs",
        "ProjectDirs",
        "UserDirs",
        "data_local_dir",
        "APPDATA",
        "config_dir()",
    ];
    let mut bad = vec![];
    for f in files {
        let rel = f
            .strip_prefix(root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if allowed.iter().any(|a| rel.ends_with(a)) || rel.contains("/examples/") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        for n in needles {
            if text.contains(n) {
                bad.push(format!("{rel}: {n}"));
            }
        }
    }
    assert!(bad.is_empty(), "use lowk_platform::dirs instead: {bad:?}");
}
