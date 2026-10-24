//! Documents in the vault: `<Folder>/Docs/<name>.<ext>` is the untouched original,
//! `<name>.md` the converted Markdown (`type: doc` frontmatter, see `doc_codec`), optional
//! `<name>.assets/`. Originals with an `.md` extension are stored as `<name>.md.orig` so they can
//! never be mistaken for a document file. Everything is plain files: the vault opens in Obsidian,
//! the index is derived, delete never hard-deletes (trash with the original and assets).
use crate::doc_codec::{self, estimate_tokens, DocFm};
use crate::util::*;
use crate::vault::{FileRec, ItemKind, TrashItem, Vault, VaultInner, FOLDER_FILE};
use crate::{fm, folder_codec};
use lowk_types::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const DOCS_SUBDIR: &str = "Docs";
const ORIG_SUFFIX: &str = ".orig";
/// External files younger than this are still being copied; adopted on a later rescan.
const SETTLE: Duration = Duration::from_millis(1000);

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Extension implied by the first bytes of a file with a missing or unknown extension.
fn sniff_ext(p: &Path) -> Option<&'static str> {
    let mut head = [0u8; 1024];
    let n = fs::File::open(p).and_then(|mut f| f.read(&mut head)).ok()?;
    let h = &head[..n];
    if h.windows(5).any(|w| w == b"%PDF-") {
        Some("pdf")
    } else if h.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if h.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else {
        None
    }
}

fn stem_of(name: &str) -> String {
    name.strip_suffix(".md").unwrap_or(name).to_string()
}

/// Stored name of an original: `<stem>.<ext>`, or `<stem>.md.orig` for Markdown originals.
fn stored_name(stem: &str, ext: &str) -> String {
    if ext.eq_ignore_ascii_case("md") {
        format!("{stem}.md{ORIG_SUFFIX}")
    } else if ext.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}.{ext}")
    }
}

/// Splits a stored original name into `(stem, ext)`; inverse of [`stored_name`].
fn split_stored(name: &str) -> (String, String) {
    if let Some(s) = name.strip_suffix(&format!(".md{ORIG_SUFFIX}")) {
        return (s.to_string(), "md".into());
    }
    match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), e.to_string()),
        _ => (name.to_string(), String::new()),
    }
}

fn display_name(fm: &DocFm) -> String {
    if !fm.original_name.is_empty() {
        fm.original_name.clone()
    } else {
        fm.source
            .strip_suffix(ORIG_SUFFIX)
            .unwrap_or(&fm.source)
            .to_string()
    }
}

/// Streams `src` to `dst` (temp file + rename), returning `(sha256 hex, bytes)`.
fn copy_hashed(src: &Path, dst: &Path) -> Result<(String, u64)> {
    let tmp = dst.with_file_name(format!(".{}.{}.tmp", file_name(dst), Ulid::new()));
    let res = (|| -> std::io::Result<(String, u64)> {
        let mut r = std::io::BufReader::with_capacity(1 << 20, fs::File::open(src)?);
        let mut w = fs::File::create(&tmp)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut total = 0u64;
        loop {
            let n = r.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            w.write_all(&buf[..n])?;
            total += n as u64;
        }
        w.sync_all()?;
        fs::rename(&tmp, dst)?;
        Ok((hex(&h.finalize()), total))
    })();
    res.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.into()
    })
}

fn sha256_file(p: &Path) -> Result<String> {
    let mut r = std::io::BufReader::with_capacity(1 << 20, fs::File::open(p)?);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn now() -> String {
    fmt_rfc3339(::time::OffsetDateTime::now_utc())
}

/// First free stem such that `<stem>.md`, the stored original and `<stem>.assets` are all absent.
fn unique_stem(dir: &Path, base: &str, ext: &str) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                base.to_string()
            } else {
                format!("{base} ({n})")
            }
        })
        .find(|s| {
            !dir.join(format!("{s}.md")).exists()
                && !dir.join(stored_name(s, ext)).exists()
                && !dir.join(format!("{s}.assets")).exists()
        })
        .expect("unbounded range")
}

impl VaultInner {
    fn docs_dir_of(&self, folder: &FolderRef) -> Result<PathBuf> {
        Ok(self.root.join(self.folder_dir(folder)?).join(DOCS_SUBDIR))
    }

    fn doc_rec(&self, id: &DocId) -> Result<FileRec> {
        let rec = self.find(id)?;
        if rec.kind != ItemKind::Doc {
            return Err(err(ErrorCode::NotFound, format!("{id} is not a document")));
        }
        Ok(rec)
    }

    fn load_doc(&self, rec: &FileRec) -> Result<(DocFm, String)> {
        let text = fs::read_to_string(self.root.join(&rec.rel))?;
        doc_codec::parse(&text).ok_or_else(|| {
            err(
                ErrorCode::FormatInvalid,
                format!("{} is not a document file", rec.rel),
            )
        })
    }

    fn write_doc(&self, rec: &FileRec, d: &DocFm, body: &str) -> Result<()> {
        atomic_write(&self.root.join(&rec.rel), doc_codec::render(d, body).as_bytes())
    }

    fn to_document(&self, rec: &FileRec, d: &DocFm) -> Result<Document> {
        let id: DocId =
            d.id.parse()
                .map_err(|_| err(ErrorCode::FormatInvalid, "bad document id"))?;
        let parent = Path::new(&rec.rel)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let original_rel = rel_string(&self.root, &self.root.join(parent).join(&d.source));
        let size = fs::metadata(self.root.join(&original_rel))
            .map(|m| m.len())
            .unwrap_or(0);
        Ok(Document {
            id,
            folder: rec.folder.clone(),
            title: d.title.clone(),
            kind: d.kind,
            file_name: display_name(d),
            md_path: rec.rel.clone(),
            original_path: original_rel,
            size_bytes: size,
            pages: d.pages,
            status: d.status,
            error: d.error.clone(),
            warnings: d.warnings.clone(),
            pinned: d.pinned,
            tokens: d.tokens,
            tags: d.tags.clone(),
            added: d.added.clone(),
            source_sha256: d.source_sha256.clone(),
            converter: d.converter.clone(),
            deleted: rec.deleted,
        })
    }

    /// Reads, mutates and rewrites one document file; returns the new state.
    fn mutate_doc(
        &self,
        id: &DocId,
        f: impl FnOnce(&mut DocFm, &mut String) -> Result<()>,
    ) -> Result<Document> {
        let rec = self.doc_rec(id)?;
        let (mut d, mut body) = self.load_doc(&rec)?;
        f(&mut d, &mut body)?;
        self.write_doc(&rec, &d, &body)?;
        self.resync()?;
        self.to_document(&rec, &d)
    }

    /// Absolute paths of the converted Markdown and of the stored original.
    pub fn doc_paths(&self, id: &DocId) -> Result<(PathBuf, PathBuf)> {
        let rec = self.doc_rec(id)?;
        let (d, _) = self.load_doc(&rec)?;
        let md = self.root.join(&rec.rel);
        let orig = md.with_file_name(&d.source);
        Ok((md, orig))
    }

    pub fn list_docs(&self, folder: &FolderRef) -> Result<Vec<Document>> {
        let mut out = vec![];
        let want_deleted = *folder == FolderRef::Trash;
        for rec in self.scan()?.files {
            if rec.kind != ItemKind::Doc || rec.deleted != want_deleted {
                continue;
            }
            if !want_deleted && rec.folder != *folder {
                continue;
            }
            if let Ok((d, _)) = self.load_doc(&rec) {
                out.push(self.to_document(&rec, &d)?);
            }
        }
        out.sort_by(|a, b| {
            a.added
                .cmp(&b.added)
                .then(a.title.cmp(&b.title))
                .then(a.id.cmp(&b.id))
        });
        Ok(out)
    }

    pub fn get_doc(&self, id: &DocId) -> Result<Document> {
        let rec = self.doc_rec(id)?;
        let (d, _) = self.load_doc(&rec)?;
        self.to_document(&rec, &d)
    }

    // ---- import ---------------------------------------------------------

    pub fn add_doc(&self, n: &NewDocument) -> Result<Document> {
        let _g = self.op_lock();
        let src = Path::new(&n.source_path);
        let meta = fs::metadata(src).map_err(|e| {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                ErrorCode::NotFound
            } else {
                ErrorCode::Io
            };
            err(code, format!("{}: {e}", src.display()))
        })?;
        if !meta.is_file() {
            return Err(err(ErrorCode::InvalidArgument, "not a regular file"));
        }
        let ext = src
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        // the extension decides (any case); a missing or unknown one falls back to the file's magic bytes
        let (kind, ext) = match DocKind::from_extension(&ext) {
            Some(k) => (k, ext),
            None => match sniff_ext(src) {
                Some(e) => (
                    DocKind::from_extension(e).expect("sniffed ext is known"),
                    e.to_string(),
                ),
                None => {
                    return Err(AppError::new(ErrorCode::DocUnsupported)
                        .with_param("ext", &ext)
                        .with_detail(format!("no converter for .{ext}")))
                }
            },
        };
        let dir = self.docs_dir_of(&n.folder)?;
        fs::create_dir_all(&dir)?;
        let imported = file_name(src);
        let title = n
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                src.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Untitled".into())
            });
        let stem = unique_stem(&dir, &sanitize_name(&title), &ext);
        let stored = stored_name(&stem, &ext);
        let (sha, _) = copy_hashed(src, &dir.join(&stored))?;
        let same = self.docs_with_sha(&n.folder, &sha)?;
        if !same.is_empty() && n.on_duplicate == DupAction::Reject {
            let _ = fs::remove_file(dir.join(&stored));
            return Err(AppError::new(ErrorCode::DocDuplicate)
                .with_param("id", same[0].id)
                .with_param("title", &same[0].title)
                .with_detail(format!("{} is already in the folder", same[0].title)));
        }
        let id = Ulid::new();
        let d = doc_codec::placeholder(
            &id.to_string(),
            &title,
            kind,
            &stored,
            &imported,
            &sha,
            n.pin,
            n.tags.clone(),
            &now(),
        );
        let md_path = dir.join(format!("{stem}.md"));
        if let Err(e) = atomic_write(&md_path, doc_codec::render(&d, "").as_bytes()) {
            let _ = fs::remove_file(dir.join(&stored));
            return Err(e);
        }
        if n.on_duplicate == DupAction::Replace {
            for old in &same {
                let rec = self.doc_rec(&old.id)?;
                self.trash_doc_locked(&rec)?;
            }
        }
        self.sync_card(&n.folder)?;
        self.resync()?;
        self.get_doc(&id)
    }

    /// Live documents of `folder` whose original has this SHA-256.
    fn docs_with_sha(&self, folder: &FolderRef, sha: &str) -> Result<Vec<Document>> {
        Ok(self
            .list_docs(folder)?
            .into_iter()
            .filter(|d| !d.deleted && d.source_sha256 == sha)
            .collect())
    }

    /// For each file of an import batch: the document already in its folder with identical
    /// content, if any (the UI asks open / copy / replace before importing).
    pub fn find_duplicates(&self, files: &[NewDocument]) -> Result<Vec<DuplicateHit>> {
        let mut out = vec![];
        for (i, n) in files.iter().enumerate() {
            if matches!(n.folder, FolderRef::Trash) {
                continue;
            }
            let Ok(sha) = sha256_file(Path::new(&n.source_path)) else {
                continue;
            };
            if let Some(d) = self.docs_with_sha(&n.folder, &sha)?.into_iter().next() {
                out.push(DuplicateHit {
                    index: i as u32,
                    existing: d,
                });
            }
        }
        Ok(out)
    }

    /// Files dropped into a `Docs/` directory from outside (Finder, sync, Obsidian) get a
    /// `status: queued` placeholder next to them, exactly like an import. Called by `rescan`, so the
    /// new documents show up as `Created` changes. Returns the adopted documents.
    pub(crate) fn adopt_external_docs_locked(&self) -> Result<Vec<DocId>> {
        let sc = self.scan()?;
        let mut adopted = vec![];
        let mut dirs: Vec<(FolderRef, PathBuf)> = vec![];
        for f in &sc.folders {
            dirs.push((f.fref.clone(), self.root.join(&f.dir).join(DOCS_SUBDIR)));
        }
        for (fref, dir) in dirs {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            let dir_rel = rel_string(&self.root, &dir);
            let known: std::collections::HashSet<String> = sc
                .files
                .iter()
                .filter(|f| {
                    f.kind == ItemKind::Doc
                        && !f.deleted
                        && Path::new(&f.rel).parent() == Some(Path::new(&dir_rel))
                })
                .filter_map(|f| self.load_doc(f).ok().map(|(d, _)| d.source))
                .collect();
            let mut entries: Vec<_> = rd.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            let mut touched = false;
            for e in entries {
                let name = e.file_name().to_string_lossy().into_owned();
                let path = e.path();
                if name.starts_with('.') || name.starts_with("~$") || !path.is_file() || known.contains(&name)
                {
                    continue;
                }
                let ext = path
                    .extension()
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // `.md` files are notes, not originals; `.md.orig` has the extension `orig`
                let Some(kind) = DocKind::from_extension(&ext).filter(|_| !ext.eq_ignore_ascii_case("md"))
                else {
                    continue;
                };
                let settled = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .is_some_and(|age| age >= SETTLE);
                if !settled {
                    continue;
                }
                let sha = sha256_file(&path)?;
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let md_stem = (1..)
                    .map(|n| {
                        if n == 1 {
                            stem.clone()
                        } else {
                            format!("{stem} ({n})")
                        }
                    })
                    .find(|s| !dir.join(format!("{s}.md")).exists())
                    .expect("unbounded range");
                let id = Ulid::new();
                let d = doc_codec::placeholder(
                    &id.to_string(),
                    &stem,
                    kind,
                    &name,
                    &name,
                    &sha,
                    false,
                    vec![],
                    &now(),
                );
                atomic_write(
                    &dir.join(format!("{md_stem}.md")),
                    doc_codec::render(&d, "").as_bytes(),
                )?;
                adopted.push(id);
                touched = true;
            }
            if touched {
                self.sync_card_locked(&fref)?;
            }
        }
        Ok(adopted)
    }

    /// Keeps the `## Documents` list of a folder card (`[[Docs/<name>]]`) in step with the files.
    fn sync_card(&self, folder: &FolderRef) -> Result<()> {
        self.sync_card_locked(folder)
    }

    fn sync_card_locked(&self, folder: &FolderRef) -> Result<()> {
        let FolderRef::User(fid) = folder else {
            return Ok(());
        };
        let rec = self.find_folder(fid)?;
        let Some(mut card) = rec.doc.clone() else {
            return Ok(());
        };
        let mut docs: Vec<String> = self
            .scan()?
            .files
            .iter()
            .filter(|f| f.kind == ItemKind::Doc && !f.deleted && f.folder == *folder)
            .map(|f| format!("{DOCS_SUBDIR}/{}", stem_of(&file_name(Path::new(&f.rel)))))
            .collect();
        docs.sort();
        if card.docs() != docs.as_slice() {
            card.set_docs(docs);
            atomic_write(
                &self.root.join(&rec.dir).join(FOLDER_FILE),
                folder_codec::render(&card).as_bytes(),
            )?;
        }
        Ok(())
    }

    // ---- conversion results ---------------------------------------------

    pub fn set_doc_status(
        &self,
        id: &DocId,
        status: IngestStatus,
        error: Option<AppError>,
    ) -> Result<Document> {
        let _g = self.op_lock();
        self.mutate_doc(id, |d, _| {
            d.status = status;
            d.error = match status {
                IngestStatus::Failed => Some(error.unwrap_or_else(|| AppError::new(ErrorCode::IngestFailed))),
                IngestStatus::Cancelled => error.or_else(|| Some(AppError::new(ErrorCode::IngestCancelled))),
                _ => None,
            };
            Ok(())
        })
    }

    /// Stores a conversion result. An empty result is refused: documents are never empty.
    pub fn save_converted_doc(&self, id: &DocId, doc: &IngestedDoc) -> Result<Document> {
        let _g = self.op_lock();
        if doc.md.chars().all(|c| !c.is_alphanumeric()) {
            return Err(
                AppError::new(ErrorCode::DocEmpty).with_detail("refusing to store an empty conversion")
            );
        }
        self.mutate_doc(id, |d, body| {
            d.converter = doc.converter.clone();
            d.unit = unit_of(&doc.converter).into();
            d.pages = doc.pages;
            d.warnings = doc.warnings.clone();
            d.error = None;
            d.status = IngestStatus::Indexing;
            d.tokens = estimate_tokens(&doc.md);
            *body = doc.md.clone();
            Ok(())
        })
    }

    pub fn read_doc_md(&self, id: &DocId) -> Result<String> {
        let rec = self.doc_rec(id)?;
        Ok(self.load_doc(&rec)?.1)
    }

    /// The user edited the Markdown in the app (re-index follows).
    pub fn save_doc_md(&self, id: &DocId, md: &str) -> Result<Document> {
        let _g = self.op_lock();
        self.mutate_doc(id, |d, body| {
            if d.status == IngestStatus::Converting {
                return Err(err(ErrorCode::InvalidArgument, "the document is being converted"));
            }
            *body = fm::normalize_eol(md);
            d.tokens = estimate_tokens(body);
            d.error = None;
            d.status = IngestStatus::Indexing;
            Ok(())
        })
    }

    pub fn patch_doc(&self, id: &DocId, p: &DocPatch) -> Result<Document> {
        let _g = self.op_lock();
        let rec = self.doc_rec(id)?;
        if rec.deleted {
            return Err(err(ErrorCode::InvalidArgument, "restore the document first"));
        }
        if let Some(to) = &p.folder {
            if *to != rec.folder {
                self.move_doc_locked(&rec, to)?;
            }
        }
        // the move changed the path: look the record up again
        let rec = self.doc_rec(id)?;
        let (mut d, body) = self.load_doc(&rec)?;
        if let Some(t) = p.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            d.title = t.to_string();
        }
        if let Some(t) = &p.tags {
            d.tags = t.clone();
        }
        if let Some(x) = p.pinned {
            d.pinned = x;
        }
        self.write_doc(&rec, &d, &body)?;
        self.resync()?;
        self.to_document(&rec, &d)
    }

    fn move_doc_locked(&self, rec: &FileRec, to: &FolderRef) -> Result<()> {
        let (mut d, body) = self.load_doc(rec)?;
        let new_dir = self.docs_dir_of(to)?;
        fs::create_dir_all(&new_dir)?;
        let src_md = self.root.join(&rec.rel);
        let old_dir = src_md.parent().map(Path::to_path_buf).unwrap_or_default();
        let old_stem = stem_of(&file_name(&src_md));
        let (orig_stem, ext) = split_stored(&d.source);
        let stem = unique_stem(&new_dir, &old_stem, &ext);
        let new_orig = if orig_stem == old_stem {
            stored_name(&stem, &ext)
        } else {
            unique_stored(&new_dir, &orig_stem, &ext)
        };
        let old_orig = old_dir.join(&d.source);
        if old_orig.exists() {
            fs::rename(&old_orig, new_dir.join(&new_orig))?;
        }
        let old_assets = old_dir.join(format!("{old_stem}.assets"));
        if old_assets.is_dir() {
            fs::rename(&old_assets, new_dir.join(format!("{stem}.assets")))?;
        }
        d.source = new_orig;
        let body = if stem != old_stem {
            body.replace(&format!("![[{old_stem}.assets/"), &format!("![[{stem}.assets/"))
        } else {
            body
        };
        atomic_write(
            &new_dir.join(format!("{stem}.md")),
            doc_codec::render(&d, &body).as_bytes(),
        )?;
        fs::remove_file(&src_md)?;
        self.sync_card_locked(&rec.folder)?;
        self.sync_card_locked(to)?;
        Ok(())
    }

    // ---- trash ----------------------------------------------------------

    /// Moves the Markdown, the original and the assets into `.lowk/trash/<id>/`.
    pub(crate) fn trash_doc_locked(&self, rec: &FileRec) -> Result<()> {
        let (d, _) = self.load_doc(rec)?;
        let md = self.root.join(&rec.rel);
        let dir = md.parent().map(Path::to_path_buf).unwrap_or_default();
        let stem = stem_of(&file_name(&md));
        let tdir = self.trash_root().join(&d.id);
        fs::create_dir_all(&tdir)?;
        let mut names = vec![file_name(&md), d.source.clone()];
        let mut moves = vec![
            (md.clone(), tdir.join(&names[0])),
            (dir.join(&d.source), tdir.join(&d.source)),
        ];
        let assets = format!("{stem}.assets");
        if dir.join(&assets).is_dir() {
            moves.push((dir.join(&assets), tdir.join(&assets)));
            names.push(assets);
        }
        for (s, t) in moves {
            if s.exists() {
                fs::rename(s, t)?;
            }
        }
        let item = TrashItem {
            id: rec.id,
            kind: ItemKind::Doc,
            title: d.title.clone(),
            orig_path: rec.rel.clone(),
            deleted_at: now(),
            names,
        };
        let json = serde_json::to_vec_pretty(&item).map_err(|e| err(ErrorCode::Io, e.to_string()))?;
        atomic_write(&self.trash_root().join(format!("{}.trash.json", rec.id)), &json)?;
        self.resync()?;
        self.sync_card_locked(&rec.folder)?;
        self.resync()
    }

    /// Restores a trashed document with its original and assets (name collision: numeric suffix).
    pub(crate) fn restore_doc_locked(&self, item: &TrashItem) -> Result<()> {
        let tdir = self.trash_root().join(item.id.to_string());
        let md_name = &item.names[0];
        let orig = self.root.join(&item.orig_path);
        let parent = orig.parent().unwrap_or(&self.root).to_path_buf();
        fs::create_dir_all(&parent)?;
        let old_stem = stem_of(md_name);
        let md_in_trash = tdir.join(md_name);
        let text = fs::read_to_string(&md_in_trash)?;
        let (mut d, body) = doc_codec::parse(&text)
            .ok_or_else(|| err(ErrorCode::FormatInvalid, "trashed document is unreadable"))?;
        let (orig_stem, ext) = split_stored(&d.source);
        let stem = unique_stem(&parent, &old_stem, &ext);
        let new_orig = if orig_stem == old_stem {
            stored_name(&stem, &ext)
        } else {
            unique_stored(&parent, &orig_stem, &ext)
        };
        if tdir.join(&d.source).exists() {
            fs::rename(tdir.join(&d.source), parent.join(&new_orig))?;
        }
        if let Some(a) = item.names.get(2) {
            fs::rename(tdir.join(a), parent.join(format!("{stem}.assets")))?;
        }
        d.source = new_orig;
        let body = if stem != old_stem {
            body.replace(&format!("![[{old_stem}.assets/"), &format!("![[{stem}.assets/"))
        } else {
            body
        };
        atomic_write(
            &parent.join(format!("{stem}.md")),
            doc_codec::render(&d, &body).as_bytes(),
        )?;
        let _ = fs::remove_dir_all(&tdir);
        fs::remove_file(self.trash_root().join(format!("{}.trash.json", item.id)))?;
        self.resync()?;
        if let Some(rec) = self.scan()?.files.iter().find(|f| f.id == item.id) {
            self.sync_card_locked(&rec.folder)?;
        }
        self.resync()
    }

    pub fn delete_doc(&self, id: &DocId) -> Result<()> {
        let _g = self.op_lock();
        let rec = self.doc_rec(id)?;
        if rec.deleted {
            return Ok(());
        }
        self.trash_doc_locked(&rec)
    }

    pub fn restore_doc(&self, id: &DocId) -> Result<Document> {
        {
            let _g = self.op_lock();
            let item = self
                .trash_items()?
                .into_iter()
                .find(|i| i.id == *id && i.kind == ItemKind::Doc)
                .ok_or_else(|| err(ErrorCode::NotFound, format!("{id} is not in the trash")))?;
            self.restore_doc_locked(&item)?;
        }
        self.get_doc(id)
    }

    // ---- budgets --------------------------------------------------------

    /// Pinned-documents meter of a folder (`docs_pinned_usage`).
    pub fn pinned_usage(&self, folder: &FolderRef, budget_tokens: u32) -> Result<PinnedUsage> {
        let docs = self.list_docs(folder)?;
        let pinned: Vec<&Document> = docs.iter().filter(|d| d.pinned).collect();
        let tokens: u32 = pinned
            .iter()
            .map(|d| d.tokens)
            .fold(0u32, |a, b| a.saturating_add(b));
        Ok(PinnedUsage {
            count: pinned.len() as u32,
            tokens,
            budget_tokens,
            over_budget: tokens > budget_tokens,
        })
    }

    // ---- repair after merges --------------------------------------------

    /// After an import merge, names may have received numeric suffixes independently
    /// (`spec (2).md` next to `spec (2).pdf`): point `source:` at the sibling that exists.
    pub(crate) fn repair_doc_sources(&self) -> Result<usize> {
        let mut fixed = 0;
        for rec in self
            .scan()?
            .files
            .into_iter()
            .filter(|f| f.kind == ItemKind::Doc && !f.deleted)
        {
            let Ok((mut d, body)) = self.load_doc(&rec) else {
                continue;
            };
            let md = self.root.join(&rec.rel);
            let dir = md.parent().map(Path::to_path_buf).unwrap_or_default();
            if dir.join(&d.source).exists() {
                continue;
            }
            let stem = stem_of(&file_name(&md));
            let (_, ext) = split_stored(&d.source);
            let candidate = stored_name(&stem, &ext);
            if dir.join(&candidate).exists() {
                d.source = candidate;
                atomic_write(&md, doc_codec::render(&d, &body).as_bytes())?;
                fixed += 1;
            }
        }
        if fixed > 0 {
            self.resync()?;
        }
        Ok(fixed)
    }
}

fn unique_stored(dir: &Path, base: &str, ext: &str) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                base.to_string()
            } else {
                format!("{base} ({n})")
            }
        })
        .map(|s| stored_name(&s, ext))
        .find(|n| !dir.join(n).exists())
        .expect("unbounded range")
}

/// `page` | `slide` | `sheet`: what `<!-- p:N -->` counts for a converter (mirrors `lowk-ingest`).
fn unit_of(converter: &str) -> &'static str {
    match converter {
        "pptx" | "keynote-pdf" => "slide",
        "xlsx" | "csv" => "sheet",
        _ => "page",
    }
}

/// `DocumentStore` over a vault. A wrapper (not an impl on `Vault`) because `Vault` already has
/// call-oriented `save_md` / `delete` methods that must keep resolving unambiguously.
#[derive(Clone, Debug)]
pub struct VaultDocs(pub Vault);

impl Vault {
    pub fn docs(&self) -> VaultDocs {
        VaultDocs(self.clone())
    }
}

impl DocumentStore for VaultDocs {
    fn list_docs(&self, folder: &FolderRef) -> Result<Vec<Document>> {
        VaultInner::list_docs(&self.0, folder)
    }
    fn get_doc(&self, id: &DocId) -> Result<Document> {
        VaultInner::get_doc(&self.0, id)
    }
    fn add_doc(&self, n: &NewDocument) -> Result<Document> {
        VaultInner::add_doc(&self.0, n)
    }
    fn set_status(&self, id: &DocId, status: IngestStatus, error: Option<AppError>) -> Result<Document> {
        self.0.set_doc_status(id, status, error)
    }
    fn save_converted(&self, id: &DocId, doc: &IngestedDoc) -> Result<Document> {
        self.0.save_converted_doc(id, doc)
    }
    fn read_md(&self, id: &DocId) -> Result<String> {
        self.0.read_doc_md(id)
    }
    fn save_md(&self, id: &DocId, md: &str) -> Result<Document> {
        self.0.save_doc_md(id, md)
    }
    fn patch_doc(&self, id: &DocId, p: &DocPatch) -> Result<Document> {
        VaultInner::patch_doc(&self.0, id, p)
    }
    fn delete_doc(&self, id: &DocId) -> Result<()> {
        VaultInner::delete_doc(&self.0, id)
    }
    fn restore_doc(&self, id: &DocId) -> Result<Document> {
        VaultInner::restore_doc(&self.0, id)
    }
}

#[cfg(test)]
mod sniff_tests {
    use super::*;

    #[test]
    fn pdf_without_a_useful_extension_is_recognised_by_content() {
        let d = std::env::temp_dir().join(format!("lowk-sniff-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("Академ справка.download");
        std::fs::write(&p, b"\xEF\xBB\xBF%PDF-1.4\n1 0 obj").unwrap();
        assert_eq!(sniff_ext(&p), Some("pdf"));
        std::fs::write(&p, b"just text").unwrap();
        assert_eq!(sniff_ext(&p), None);
        assert_eq!(DocKind::from_extension("PDF"), Some(DocKind::Pdf));
        let _ = std::fs::remove_dir_all(&d);
    }
}
