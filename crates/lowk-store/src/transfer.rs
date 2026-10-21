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
        self.repair_doc_sources()?;
        Ok(rep)
    }
}

// ---- zip export / import (folder or whole vault) -------------------------------------------

const MAX_ENTRIES: usize = 500_000;
const MAX_TOTAL: u64 = 64 * 1024 * 1024 * 1024;

fn already_compressed(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "pdf"
                | "png"
                | "jpg"
                | "jpeg"
                | "heic"
                | "webp"
                | "zip"
                | "docx"
                | "pptx"
                | "xlsx"
                | "key"
                | "orig"
        )
    })
}

fn zip_err(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::ArchiveInvalid).with_detail(e.to_string())
}

impl Vault {
    fn zip_files(&self, base: &Path, files: &[PathBuf], dest: &Path) -> Result<usize> {
        if dest.exists() {
            return Err(err(ErrorCode::AlreadyExists, "destination exists"));
        }
        if dest.starts_with(&self.root) {
            return Err(err(ErrorCode::InvalidArgument, "destination is inside the vault"));
        }
        let tmp = dest.with_extension("zip.part");
        let res = (|| -> Result<usize> {
            let mut z = zip::ZipWriter::new(fs::File::create(&tmp)?);
            for f in files {
                let rel = rel_string(base, f);
                let opts =
                    zip::write::SimpleFileOptions::default().compression_method(if already_compressed(f) {
                        zip::CompressionMethod::Stored
                    } else {
                        zip::CompressionMethod::Deflated
                    });
                z.start_file(rel, opts).map_err(zip_err)?;
                std::io::copy(&mut fs::File::open(f)?, &mut z)?;
            }
            z.finish().map_err(zip_err)?;
            Ok(files.len())
        })();
        match res {
            Ok(n) => {
                fs::rename(&tmp, dest)?;
                Ok(n)
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    /// One folder (card, calls, documents with originals and assets) as `<Folder>/...` in a zip.
    /// The layout is the vault layout, so the archive unzips into an Obsidian vault as is.
    pub fn export_folder_zip(&self, id: &Ulid, dest: &Path) -> Result<usize> {
        let _g = self.op_lock();
        let rec = self.find_folder(id)?;
        let dir = self.root.join(&rec.dir);
        let mut files = vec![];
        files_in(&dir, false, &mut files)?;
        files.sort();
        self.zip_files(&self.root, &files, dest)
    }

    /// The whole vault as a zip (`.lowk/` only when `include_service`).
    pub fn export_vault_zip(&self, dest: &Path, include_service: bool) -> Result<usize> {
        let _g = self.op_lock();
        let mut files = vec![];
        files_in(&self.root, !include_service, &mut files)?;
        files.sort();
        self.zip_files(&self.root, &files, dest)
    }

    /// Merges a folder or vault zip into this vault: never overwrites, ids reassigned on clash.
    /// Hostile archives (path traversal, absolute paths, symlinks, bombs) are `ArchiveInvalid`.
    pub fn import_zip(&self, src: &Path) -> Result<ImportReport> {
        let f = fs::File::open(src)?;
        let mut z = zip::ZipArchive::new(f).map_err(zip_err)?;
        if z.len() > MAX_ENTRIES {
            return Err(zip_err("too many entries"));
        }
        let stage = self
            .root
            .join(SERVICE_DIR)
            .join("tmp")
            .join(format!("import-{}", Ulid::new()));
        fs::create_dir_all(&stage)?;
        let res = (|| -> Result<ImportReport> {
            let mut total = 0u64;
            let mut root_card = false;
            let mut names = vec![];
            for i in 0..z.len() {
                let mut e = z.by_index(i).map_err(zip_err)?;
                if e.is_dir() {
                    continue;
                }
                if e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                    return Err(zip_err("symlink entry"));
                }
                let name = e.name().map_err(zip_err)?.replace('\\', "/");
                let p = Path::new(&name);
                if name.starts_with('/')
                    || name.as_bytes().get(1) == Some(&b':')
                    || p.components()
                        .any(|c| !matches!(c, std::path::Component::Normal(_)))
                {
                    return Err(zip_err(format!("unsafe path {name:?}")));
                }
                total = total.saturating_add(e.size());
                if total > MAX_TOTAL {
                    return Err(zip_err("archive is too large"));
                }
                if name == FOLDER_FILE {
                    root_card = true;
                }
                names.push(name.clone());
                let out = stage.join(p);
                if let Some(d) = out.parent() {
                    fs::create_dir_all(d)?;
                }
                let mut w = fs::File::create(&out)?;
                let declared = e.size();
                let copied = std::io::copy(
                    &mut std::io::Read::take(&mut e, declared.saturating_add(1)),
                    &mut w,
                )
                .map_err(zip_err)?;
                if copied > declared {
                    return Err(zip_err("entry is larger than declared"));
                }
            }
            if names.is_empty() {
                return Err(zip_err("archive is empty"));
            }
            let mut top = stage.clone();
            if root_card {
                // folder contents zipped without the folder directory
                let stem = src
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Imported".into());
                let wrap = stage.join("__wrap").join(sanitize_name(&stem));
                for n in &names {
                    let to = wrap.join(n);
                    if let Some(d) = to.parent() {
                        fs::create_dir_all(d)?;
                    }
                    fs::rename(stage.join(n), to)?;
                }
                top = stage.join("__wrap");
            }
            self.import_vault(&top)
        })();
        let _ = fs::remove_dir_all(&stage);
        res
    }
}
