//! Windows `\\?\` (verbatim) path handling. `canonicalize` on Windows returns verbatim paths, which many
//! APIs (Explorer reveal, WinRT `StorageFile`, shell, WebView asset URLs, prefix comparisons with
//! user-supplied paths) reject or treat as different from the plain form.
use std::path::{Path, PathBuf};

/// Drops a leading `\\?\` (and maps `\\?\UNC\server\share` to `\\server\share`). Other paths pass through.
/// Pure string handling: identical on every OS, so it is unit-tested everywhere.
pub fn strip_verbatim(p: &str) -> String {
    if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = p.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        p.to_owned()
    }
}

/// `canonicalize` without the verbatim prefix (the app and worker manifests are `longPathAware`, so the plain
/// form works beyond 260 characters too).
pub fn canonicalize_plain(p: &Path) -> std::io::Result<PathBuf> {
    let c = p.canonicalize()?;
    Ok(simplified(&c))
}

/// [`strip_verbatim`] for a `Path`; non-UTF-8 paths are returned unchanged.
pub fn simplified(p: &Path) -> PathBuf {
    match p.to_str() {
        Some(s) => PathBuf::from(strip_verbatim(s)),
        None => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_verbatim_prefixes() {
        assert_eq!(strip_verbatim(r"\\?\C:\Users\me\Lowk"), r"C:\Users\me\Lowk");
        assert_eq!(strip_verbatim(r"\\?\UNC\srv\share\x"), r"\\srv\share\x");
        assert_eq!(strip_verbatim(r"C:\plain"), r"C:\plain");
        assert_eq!(strip_verbatim("/home/me/Lowk"), "/home/me/Lowk");
    }

    #[test]
    fn canonicalize_plain_matches_canonicalize_on_unix_paths() {
        let d = std::env::temp_dir();
        assert_eq!(
            canonicalize_plain(&d).unwrap(),
            simplified(&d.canonicalize().unwrap())
        );
    }
}
