//! Export / import = plain folder copy (architecture 4.1). Export skips the
//! machine-local `.lowk/` unless asked; import merges another vault in and
//! resolves id / name conflicts without ever overwriting.
use crate::fm;
use crate::util::*;
use crate::vault::{Vault, FOLDER_FILE, SERVICE_DIR};
use lowk_types::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub files_copied: usize,
    /// Identical file already present.
    pub skipped: usize,
    /// Imported file got a fresh id because its id clashed with different content.
    pub ids_reassigned: usize,
    pub folders_merged: usize,
}

fn files_in(dir: &Path, skip_service: bool, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        let p = e.path();
        if (skip_service && name == SERVICE_DIR) || name.ends_with(".tmp") && name.starts_with('.') {
            continue;
        }
        if name.starts_with('.') && name != SERVICE_DIR {
            continue;
        }
        if p.is_dir() {
            files_in(&p, false, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

fn md_id(text: &str) -> Option<Ulid> {
    let norm = fm::normalize_eol(text);
    let (f, _) = fm::split(&norm);
    f?.get("id").map(fm::scalar)?.parse().ok()
}

fn with_new_id(text: &str, id: &Ulid) -> String {
    let norm = fm::normalize_eol(text);
    let (f, body) = fm::split(&norm);
    match f {
        Some(mut f) => {
            f.set_ordered("id", id.to_string(), &["lowk", "type", "id"]);
            format!("{}{}", f.render(), body)
        }
        None => norm,
    }
}

impl Vault {
    /// Copy the vault to `dest` (must not exist or be empty). Trash and the
    /// index live in `.lowk/` and are only included when `include_service`.
    pub fn export_vault(&self, dest: &Path, include_service: bool) -> Result<usize> {
        let _g = self.op_lock();
        if dest.exists() && fs::read_dir(dest)?.next().is_some() {
            return Err(err(ErrorCode::AlreadyExists, "destination is not empty"));
        }
        if dest.starts_with(&self.root) {
            return Err(err(ErrorCode::InvalidArgument, "destination is inside the vault"));
        }
        let mut files = vec![];
        files_in(&self.root, !include_service, &mut files)?;
        for f in &files {
            let rel = f.strip_prefix(&self.root).unwrap_or(f);
            let to = dest.join(rel);
            if let Some(p) = to.parent() {
                fs::create_dir_all(p)?;
            }
            fs::copy(f, &to)?;
        }
        Ok(files.len())
    }

    /// Merge the vault at `src` into this one.
    pub fn import_vault(&self, src: &Path) -> Result<ImportReport> {
        let _g = self.op_lock();
        if !src.is_dir() {
            return Err(err(ErrorCode::VaultInvalid, "source is not a directory"));
        }
        let sc = self.scan()?;
        let mut ids: HashSet<Ulid> = sc.files.iter().map(|f| f.id).collect();
        let mut rep = ImportReport::default();
        let mut tops: Vec<_> = fs::read_dir(src)?.flatten().collect();
        tops.sort_by_key(|e| e.file_name());
        for top in tops {
            let name = top.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !top.path().is_dir() {
                continue;
            }
            let card = fs::read_to_string(top.path().join(FOLDER_FILE)).ok();
            let fid = card.as_deref().and_then(md_id);
            let existing = fid.and_then(|id| sc.folders.iter().find(|f| f.fref == FolderRef::User(id)));
            let target = if name == UNSORTED_DIR {
                self.root.join(UNSORTED_DIR)
            } else if let Some(f) = existing {
                rep.folders_merged += 1;
                self.root.join(&f.dir)
            } else {
                unique_path(&self.root, &sanitize_name(&name), "")
            };
            fs::create_dir_all(&target)?;
            let mut files = vec![];
            files_in(&top.path(), false, &mut files)?;
            for f in files {
                let rel = f.strip_prefix(top.path()).unwrap_or(&f).to_path_buf();
                let is_card = rel == Path::new(FOLDER_FILE);
                let is_md = f.extension().is_some_and(|e| e == "md");
                let bytes = fs::read(&f)?;
                let mut dst = target.join(&rel);
                let mut out = bytes.clone();
                if is_card && (existing.is_some() || dst.exists()) {
                    rep.skipped += 1;
                    continue;
                }
                if is_md && !is_card {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    if let Some(id) = md_id(&text) {
                        let same_place =
                            fs::read(&dst).ok().map(|b| hash_bytes(&b)) == Some(hash_bytes(&bytes));
                        if same_place {
                            rep.skipped += 1;
                            continue;
                        }
                        if ids.contains(&id) {
                            // clash with different content: import as a new item
                            let nid = Ulid::new();
                            out = with_new_id(&text, &nid).into_bytes();
                            ids.insert(nid);
                            rep.ids_reassigned += 1;
                        } else {
                            ids.insert(id);
                        }
                    }
                } else if fs::read(&dst).ok().map(|b| hash_bytes(&b)) == Some(hash_bytes(&bytes)) {
                    rep.skipped += 1;
                    continue;
                }
                if dst.exists() {
                    let stem = dst
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let ext = dst
                        .extension()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    dst = unique_path(dst.parent().unwrap_or(&target), &stem, &ext);
                }
                if let Some(p) = dst.parent() {
                    fs::create_dir_all(p)?;
                }
                atomic_write(&dst, &out)?;
                rep.files_copied += 1;
            }
        }
        self.resync()?;
        Ok(rep)
    }
}
