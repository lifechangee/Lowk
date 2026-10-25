//! «Delete settings, models and logs» (uninstaller option, `lowk.exe --purge-user-data`).
//!
//! The targets come from [`Layout`] (the same path helpers the app writes through), so the uninstaller can never
//! forget a location. The vault (the user's notes) is never deleted: a folder that is, contains or lies inside it is
//! skipped, and the *default* vault folder is removed only when it is completely empty.
use crate::dirs::{self, Layout, BUNDLE_ID, ROOT_SUBDIRS};
use std::path::{Path, PathBuf};

/// Providers whose API keys the app stores in the OS keychain (service = bundle id).
pub const SECRET_PROVIDERS: [&str; 2] = ["openrouter", "stt-cloud"];

/// Everything outside [`Layout`] that the purge has to know.
#[derive(Clone, Debug, Default)]
pub struct Extras {
    /// WebView2 / webview profile folders (`%LOCALAPPDATA%\<bundle id>`, legacy `%APPDATA%\<bundle id>`).
    pub webview_dirs: Vec<PathBuf>,
    /// Fallback data root (`%LOCALAPPDATA%\Lowk`), which may hold data from an earlier layout.
    pub local_fallback: Option<PathBuf>,
    /// Folders that must survive (the configured vault).
    pub protected: Vec<PathBuf>,
    /// The default vault folder: removed only if it is empty.
    pub default_vault: Option<PathBuf>,
}

impl Extras {
    pub fn from_system(protected: Vec<PathBuf>) -> Self {
        let mut webview_dirs = vec![];
        if let Some(b) = directories::BaseDirs::new() {
            webview_dirs.push(b.data_local_dir().join(BUNDLE_ID));
            webview_dirs.push(b.data_dir().join(BUNDLE_ID));
        }
        Self {
            webview_dirs,
            local_fallback: dirs::local_fallback(),
            protected,
            default_vault: dirs::default_vault(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub removed: Vec<PathBuf>,
    pub kept: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
}

/// Folders removed recursively. The data root itself is not here (it may be a folder the user chose and shares with
/// other things): only its known sub-folders go, then the root if it ended up empty.
pub fn targets(l: &Layout, x: &Extras) -> Vec<PathBuf> {
    let mut t = vec![l.app_data.clone()];
    t.extend(ROOT_SUBDIRS.iter().map(|s| l.data_root.join(s)));
    t.extend(x.webview_dirs.iter().cloned());
    t.extend(x.local_fallback.iter().cloned());
    let mut seen: Vec<PathBuf> = vec![];
    for p in t {
        if !seen.contains(&p) {
            seen.push(p);
        }
    }
    seen
}

fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// Deletes the targets; `delete_secret(provider)` removes one keychain entry.
pub fn purge(l: &Layout, x: &Extras, delete_secret: &mut dyn FnMut(&str) -> Result<(), String>) -> Report {
    let mut r = Report::default();
    for t in targets(l, x) {
        if let Some(v) = x.protected.iter().find(|v| overlaps(&t, v)) {
            // A target that *contains* the vault is emptied around it; one inside it is left alone.
            tracing::warn!("purge: {} overlaps the vault {}", t.display(), v.display());
            r.kept.push(t);
            continue;
        }
        if !t.exists() {
            continue;
        }
        match std::fs::remove_dir_all(&t) {
            Ok(()) => r.removed.push(t),
            Err(e) => r.failed.push((t, e.to_string())),
        }
    }
    // The data root, when it is now empty (e.g. `<install dir>\data`).
    if l.data_root.is_dir() && !x.protected.iter().any(|v| overlaps(&l.data_root, v)) {
        match std::fs::remove_dir(&l.data_root) {
            Ok(()) => r.removed.push(l.data_root.clone()),
            Err(_) => r.kept.push(l.data_root.clone()),
        }
    }
    if let Some(v) = &x.default_vault {
        // `remove_dir` only succeeds on an empty directory: notes are never deleted.
        if v.is_dir() {
            match std::fs::remove_dir(v) {
                Ok(()) => r.removed.push(v.clone()),
                Err(_) => r.kept.push(v.clone()),
            }
        }
    }
    for p in SECRET_PROVIDERS {
        if let Err(e) = delete_secret(p) {
            r.failed.push((PathBuf::from(format!("credential:{p}")), e));
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    fn setup() -> (tempfile::TempDir, Layout, Extras) {
        let t = tempfile::tempdir().unwrap();
        let l = Layout::new(t.path().join("roaming/Lowk"), t.path().join("Programs/Lowk/data"));
        let x = Extras {
            webview_dirs: vec![
                t.path().join("local").join(BUNDLE_ID),
                t.path().join("roaming").join(BUNDLE_ID),
            ],
            local_fallback: Some(t.path().join("local/Lowk")),
            protected: vec![t.path().join("Documents/Lowk")],
            default_vault: Some(t.path().join("Documents/Lowk")),
        };
        (t, l, x)
    }

    /// The purge must cover every location the app writes to (enumerated from the path helpers).
    #[test]
    fn purge_covers_every_path_the_app_writes() {
        let (t, l, x) = setup();
        let paths = l.written_paths();
        for p in &paths {
            if p.extension().is_some() {
                touch(p);
            }
        }
        for d in [l.models(), l.engines(), l.logs()] {
            touch(&d.join("some/file.bin"));
        }
        for w in &x.webview_dirs {
            touch(&w.join("EBWebView/Default/Local Storage/x"));
        }
        touch(&x.local_fallback.clone().unwrap().join("models/m.bin"));
        let notes = t.path().join("Documents/Lowk/note.md");
        touch(&notes);

        let mut gone = vec![];
        let rep = purge(&l, &x, &mut |p| {
            gone.push(p.to_string());
            Ok(())
        });
        assert!(rep.failed.is_empty(), "{:?}", rep.failed);
        for p in &paths {
            assert!(!p.exists(), "{} survived the purge", p.display());
        }
        for w in &x.webview_dirs {
            assert!(!w.exists());
        }
        assert!(!l.data_root.exists(), "empty data root is removed");
        assert!(!x.local_fallback.unwrap().exists());
        assert_eq!(gone, SECRET_PROVIDERS);
        assert!(notes.exists(), "the vault is never touched");
    }

    #[test]
    fn custom_data_root_keeps_foreign_files() {
        let (t, mut l, x) = setup();
        l.data_root = t.path().join("D/LowkModels");
        touch(&l.models().join("m.bin"));
        touch(&l.data_root.join("mine.txt"));
        purge(&l, &x, &mut |_| Ok(()));
        assert!(!l.models().exists());
        assert!(
            l.data_root.join("mine.txt").exists(),
            "only Lowk's own folders are removed"
        );
    }

    #[test]
    fn empty_default_vault_goes_non_empty_stays() {
        let (t, l, x) = setup();
        let v = t.path().join("Documents/Lowk");
        fs::create_dir_all(&v).unwrap();
        let rep = purge(&l, &x, &mut |_| Ok(()));
        assert!(!v.exists() && rep.removed.contains(&v));
        touch(&v.join("a.md"));
        purge(&l, &x, &mut |_| Ok(()));
        assert!(v.join("a.md").exists());
    }

    #[test]
    fn a_root_inside_the_vault_is_left_alone() {
        let (t, mut l, x) = setup();
        l.data_root = t.path().join("Documents/Lowk/.lowk-data");
        touch(&l.models().join("m.bin"));
        let rep = purge(&l, &x, &mut |_| Ok(()));
        assert!(l.models().join("m.bin").exists());
        assert!(rep.kept.contains(&l.models()));
    }

    #[test]
    fn keychain_failures_are_reported_not_fatal() {
        let (_t, l, x) = setup();
        let rep = purge(&l, &x, &mut |_| Err("locked".into()));
        assert_eq!(rep.failed.len(), SECRET_PROVIDERS.len());
    }
}
