//! Vault: open / scan / CRUD / trash / rescan. The `.md` files are the source of
//! truth; everything here is derived from a directory scan (no hidden state
//! except the journal, the trash sidecars and a stat cache).
use crate::call_codec::{self, CallDoc, CallFm};
use crate::fm;
use crate::folder_codec::{self, FolderDoc};
use crate::util::*;
use crate::writer::CallWriter;
use lowk_types::*;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub const SERVICE_DIR: &str = ".lowk";
pub const FOLDER_FILE: &str = "_folder.md";
const CALLS_SUBDIR: &str = "Calls";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Call,
    Doc,
    Folder,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashItem {
    pub id: Ulid,
    pub kind: ItemKind,
    pub title: String,
    /// Original path relative to the vault root.
    pub orig_path: String,
    /// RFC 3339 (UTC).
    pub deleted_at: String,
    /// Entry names inside `.lowk/trash/<id>/`.
    pub names: Vec<String>,
}

/// Fields of `_folder.md` the UI can change; `None` = keep.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderPatch {
    pub title: Option<String>,
    pub tags: Option<Vec<String>>,
    pub allow_cloud: Option<bool>,
    /// `Some(None)` clears the default template.
    pub default_template: Option<Option<String>>,
    pub mic: Option<MicPreset>,
    pub retx: Option<RetxPreset>,
    pub instructions: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct FileRec {
    pub id: Ulid,
    pub kind: ItemKind,
    pub rel: String,
    pub folder: FolderRef,
    pub meta: Option<CallMeta>,
    pub title: String,
    pub hash: String,
    pub deleted: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct FolderRec {
    pub fref: FolderRef,
    pub dir: String,
    pub doc: Option<FolderDoc>,
    pub hash: String,
}

#[derive(Default)]
pub(crate) struct Scan {
    pub folders: Vec<FolderRec>,
    pub files: Vec<FileRec>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Known {
    rel: String,
    hash: String,
    is_folder: bool,
    deleted: bool,
}

type CacheEntry = (u64, u128, Option<FileRec>);

pub struct VaultInner {
    pub(crate) root: PathBuf,
    op: Mutex<()>,
    cache: Mutex<HashMap<PathBuf, CacheEntry>>,
    known: Mutex<HashMap<Ulid, Known>>,
    pub(crate) live: Arc<Mutex<HashSet<Ulid>>>,
}

#[derive(Clone)]
pub struct Vault(Arc<VaultInner>);

impl Deref for Vault {
    type Target = VaultInner;
    fn deref(&self) -> &VaultInner {
        &self.0
    }
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Vault({})", self.root.display())
    }
}

#[derive(Debug)]
pub struct Call {
    pub meta: CallMeta,
    pub md: String,
}

fn mtime_ns(m: &fs::Metadata) -> u128 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn walk_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if is_hidden(&name) {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            if !name.ends_with(".assets") {
                walk_md(&p, out);
            }
        } else if name.ends_with(".md") && name != FOLDER_FILE {
            out.push(p);
        }
    }
}

fn meta_of(doc: &CallDoc, rel: &str, folder: FolderRef, deleted: bool) -> Option<CallMeta> {
    let id: Ulid = doc.fm.id.parse().ok()?;
    let f = &doc.fm;
    Some(CallMeta {
        id,
        title: f.title.clone(),
        folder,
        started: f.started.clone(),
        ended: f.ended.clone(),
        duration_s: f.duration_s,
        languages: f.languages.clone(),
        tags: f.tags.clone(),
        sources: CallSources {
            me: f.source_me.clone(),
            system: f.source_system.clone(),
        },
        stt_engine: f.stt_engine.clone(),
        path: rel.to_string(),
        deleted,
    })
}

impl Vault {
    /// Open (creating when needed) a vault rooted at `p`.
    pub fn open(p: &Path) -> Result<Self> {
        fs::create_dir_all(p)?;
        // no `\\?\` prefix on Windows: the root is shown, revealed in Explorer and compared with plain paths
        let root = lowk_types::paths::canonicalize_plain(p).unwrap_or_else(|_| p.to_path_buf());
        for d in [
            root.join(SERVICE_DIR).join("trash"),
            root.join(SERVICE_DIR).join("state").join("live"),
            root.join(UNSORTED_DIR).join(CALLS_SUBDIR),
        ] {
            fs::create_dir_all(d)?;
        }
        let v = Vault(Arc::new(VaultInner {
            root,
            op: Mutex::new(()),
            cache: Mutex::new(HashMap::new()),
            known: Mutex::new(HashMap::new()),
            live: Arc::new(Mutex::new(HashSet::new())),
        }));
        v.resync()?;
        Ok(v)
    }

    pub fn begin_call(&self, m: NewCall) -> Result<CallWriter> {
        let clock = m.clock.unwrap_or_else(|| SessionClock {
            t0_mono: now_mono_ns(),
            t0_wall_ms: wall_ms(),
        });
        self.begin_call_with_clock(m, clock)
    }

    pub fn begin_call_with_clock(&self, m: NewCall, clock: SessionClock) -> Result<CallWriter> {
        let _g = self.0.op_lock();
        let dir = self.0.folder_dir(&m.folder)?;
        let name = format!("{} {}", file_stamp(&m.started), sanitize_name(&m.title));
        let calls_dir = self.0.root.join(&dir).join(CALLS_SUBDIR);
        fs::create_dir_all(&calls_dir)?;
        let path = unique_path(&calls_dir, &name, "md");
        let id = Ulid::new();
        let folder_title = match &m.folder {
            FolderRef::User(fid) => self
                .0
                .find_folder(fid)?
                .doc
                .map(|d| d.title)
                .unwrap_or(dir.clone()),
            _ => dir.clone(),
        };
        let w = CallWriter::create(self, id, m, clock, path, folder_title)?;
        self.0.resync()?;
        Ok(w)
    }

    pub(crate) fn arc(&self) -> Arc<VaultInner> {
        self.0.clone()
    }

    pub fn create_in(p: &Path) -> Result<Self> {
        Self::open(p)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl VaultInner {
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn service(&self) -> PathBuf {
        self.root.join(SERVICE_DIR)
    }
    pub(crate) fn trash_root(&self) -> PathBuf {
        self.root.join(TRASH_DIR)
    }
    pub(crate) fn live_dir(&self) -> PathBuf {
        self.service().join("state").join("live")
    }

    // ---- scanning -------------------------------------------------------

    fn read_rec(&self, path: &Path, folder: &FolderRef, deleted: bool, rel: String) -> Option<FileRec> {
        let md = fs::metadata(path).ok()?;
        let (size, mt) = (md.len(), mtime_ns(&md));
        let fresh = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos().saturating_sub(mt) < Duration::from_secs(2).as_nanos())
            .unwrap_or(true);
        if !fresh {
            if let Some((s, m, rec)) = self.cache.lock().get(path) {
                if *s == size && *m == mt {
                    return rec.clone().map(|mut r| {
                        r.rel = rel;
                        r.folder = folder.clone();
                        r.deleted = deleted;
                        if let Some(m) = &mut r.meta {
                            m.path = r.rel.clone();
                            m.folder = folder.clone();
                            m.deleted = deleted;
                        }
                        r
                    });
                }
            }
        }
        let bytes = fs::read(path).ok()?;
        let hash = hash_bytes(&bytes);
        let text = fm::normalize_eol(&String::from_utf8_lossy(&bytes));
        let (front, _) = fm::split(&text);
        let rec = front.and_then(|f| {
            let ty = f.get("type").map(fm::scalar).unwrap_or_default();
            let id: Ulid = f.get("id").map(fm::scalar)?.parse().ok()?;
            match ty.as_str() {
                "call" => {
                    let doc = call_codec::parse(&text);
                    let meta = meta_of(&doc, &rel, folder.clone(), deleted)?;
                    Some(FileRec {
                        id,
                        kind: ItemKind::Call,
                        title: doc.fm.title.clone(),
                        rel: rel.clone(),
                        folder: folder.clone(),
                        meta: Some(meta),
                        hash: hash.clone(),
                        deleted,
                    })
                }
                "doc" => Some(FileRec {
                    id,
                    kind: ItemKind::Doc,
                    title: f.get("title").map(fm::scalar).unwrap_or_default(),
                    rel: rel.clone(),
                    folder: folder.clone(),
                    meta: None,
                    hash: hash.clone(),
                    deleted,
                }),
                _ => None,
            }
        });
        self.cache
            .lock()
            .insert(path.to_path_buf(), (size, mt, rec.clone()));
        rec
    }

    pub(crate) fn scan(&self) -> Result<Scan> {
        let mut sc = Scan::default();
        let mut seen: HashSet<Ulid> = HashSet::new();
        let mut dirs: Vec<_> = fs::read_dir(&self.root)?.flatten().collect();
        dirs.sort_by_key(|e| e.file_name());
        for e in dirs {
            let name = e.file_name().to_string_lossy().into_owned();
            if is_hidden(&name) || !e.path().is_dir() {
                continue;
            }
            let dir = e.path();
            let (fref, doc, hash) = if name == UNSORTED_DIR {
                (FolderRef::Unsorted, None, String::new())
            } else {
                match self.load_or_adopt_folder(&dir, &name)? {
                    Some((id, doc, hash)) => (FolderRef::User(id), Some(doc), hash),
                    None => continue,
                }
            };
            if let FolderRef::User(id) = &fref {
                if !seen.insert(*id) {
                    continue;
                }
            }
            let mut files = vec![];
            walk_md(&dir, &mut files);
            for f in files {
                let rel = rel_string(&self.root, &f);
                if let Some(rec) = self.read_rec(&f, &fref, false, rel) {
                    if seen.insert(rec.id) {
                        sc.files.push(rec);
                    }
                }
            }
            sc.folders.push(FolderRec {
                fref,
                dir: name,
                doc,
                hash,
            });
        }
        // trash
        let tr = self.trash_root();
        for item in self.trash_items()? {
            let base = tr.join(item.id.to_string());
            let mut files = vec![];
            walk_md(&base, &mut files);
            for f in files {
                let rel = rel_string(&self.root, &f);
                if let Some(rec) = self.read_rec(&f, &FolderRef::Trash, true, rel) {
                    if seen.insert(rec.id) {
                        sc.files.push(rec);
                    }
                }
            }
        }
        Ok(sc)
    }

    /// Load `_folder.md`; adopt plain directories (writes a new `_folder.md`).
    fn load_or_adopt_folder(&self, dir: &Path, name: &str) -> Result<Option<(Ulid, FolderDoc, String)>> {
        let f = dir.join(FOLDER_FILE);
        let (mut doc, existing) = match fs::read_to_string(&f) {
            Ok(t) => (folder_codec::parse(&t), true),
            Err(_) => (FolderDoc::new("", name), false),
        };
        let id = match doc.id.parse::<Ulid>() {
            Ok(id) => id,
            Err(_) => {
                let id = Ulid::new();
                doc.id = id.to_string();
                if doc.title.is_empty() {
                    doc.title = name.to_string();
                }
                let _ = existing;
                atomic_write(&f, folder_codec::render(&doc).as_bytes())?;
                id
            }
        };
        let hash = hash_bytes(&fs::read(&f).unwrap_or_default());
        Ok(Some((id, doc, hash)))
    }

    /// Re-scan and replace the "known" snapshot without reporting changes
    /// (used after our own mutations).
    pub(crate) fn resync(&self) -> Result<()> {
        let sc = self.scan()?;
        *self.known.lock() = Self::snapshot(&sc);
        Ok(())
    }

    fn snapshot(sc: &Scan) -> HashMap<Ulid, Known> {
        let mut m = HashMap::new();
        for f in &sc.folders {
            if let FolderRef::User(id) = &f.fref {
                m.insert(
                    *id,
                    Known {
                        rel: f.dir.clone(),
                        hash: f.hash.clone(),
                        is_folder: true,
                        deleted: false,
                    },
                );
            }
        }
        for f in &sc.files {
            m.insert(
                f.id,
                Known {
                    rel: f.rel.clone(),
                    hash: f.hash.clone(),
                    is_folder: false,
                    deleted: f.deleted,
                },
            );
        }
        m
    }

    /// Reconcile with the disk: report everything that changed since the last
    /// API mutation or `rescan` (external edits, renames, sync). Use at start
    /// and after watcher events.
    pub fn rescan(&self) -> Result<Vec<StoreChange>> {
        let _g = self.op.lock();
        // files dropped into a Docs/ directory become queued documents (reported as Created below)
        if let Err(e) = self.adopt_external_docs_locked() {
            tracing::warn!("adopting external documents failed: {e}");
        }
        let sc = self.scan()?;
        let now = Self::snapshot(&sc);
        let mut known = self.known.lock();
        let mut out = vec![];
        let mut push = |kind, id, is_folder| out.push(StoreChange { kind, id, is_folder });
        for (id, n) in &now {
            match known.get(id) {
                None => push(StoreChangeKind::Created, *id, n.is_folder),
                Some(o) if o.deleted != n.deleted => push(
                    if n.deleted {
                        StoreChangeKind::Trashed
                    } else {
                        StoreChangeKind::Restored
                    },
                    *id,
                    n.is_folder,
                ),
                Some(o) if o.rel != n.rel => push(StoreChangeKind::Moved, *id, n.is_folder),
                Some(o) if o.hash != n.hash => push(StoreChangeKind::Updated, *id, n.is_folder),
                _ => {}
            }
        }
        for (id, o) in known.iter() {
            if !now.contains_key(id) {
                push(StoreChangeKind::Removed, *id, o.is_folder);
            }
        }
        *known = now;
        out.sort_by_key(|c| c.id);
        Ok(out)
    }

    pub(crate) fn find(&self, id: &Ulid) -> Result<FileRec> {
        self.scan()?
            .files
            .into_iter()
            .find(|f| f.id == *id)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("no item {id}")))
    }

    pub(crate) fn find_folder(&self, id: &Ulid) -> Result<FolderRec> {
        self.scan()?
            .folders
            .into_iter()
            .find(|f| f.fref == FolderRef::User(*id))
            .ok_or_else(|| err(ErrorCode::NotFound, format!("no folder {id}")))
    }

    pub(crate) fn folder_dir(&self, f: &FolderRef) -> Result<String> {
        match f {
            FolderRef::Unsorted => Ok(UNSORTED_DIR.into()),
            FolderRef::Trash => Err(err(
                ErrorCode::InvalidArgument,
                "cannot place items in the trash directly",
            )),
            FolderRef::User(id) => Ok(self.find_folder(id)?.dir),
        }
    }

    // ---- folders --------------------------------------------------------

    pub fn folders(&self) -> Result<Vec<FolderMeta>> {
        let sc = self.scan()?;
        let count = |r: &FolderRef| {
            sc.files
                .iter()
                .filter(|f| f.kind == ItemKind::Call && f.folder == *r)
                .count() as u32
        };
        let mut out = vec![];
        for f in &sc.folders {
            let (title, tags, allow_cloud, default_template, mic, retx) = match &f.doc {
                Some(d) => (
                    d.title.clone(),
                    d.tags.clone(),
                    d.allow_cloud,
                    d.default_template.clone(),
                    d.mic,
                    d.retx,
                ),
                None => (
                    "Unsorted".into(),
                    vec![],
                    true,
                    None,
                    MicPreset::Inherit,
                    RetxPreset::Inherit,
                ),
            };
            out.push(FolderMeta {
                id: f.fref.clone(),
                title,
                tags,
                allow_cloud,
                default_template,
                mic,
                retx,
                path: f.dir.clone(),
                call_count: count(&f.fref),
            });
        }
        out.push(FolderMeta {
            id: FolderRef::Trash,
            title: "Trash".into(),
            tags: vec![],
            allow_cloud: false,
            default_template: None,
            mic: MicPreset::Inherit,
            retx: RetxPreset::Inherit,
            path: TRASH_DIR.into(),
            call_count: count(&FolderRef::Trash),
        });
        Ok(out)
    }

    pub fn create_folder(&self, f: NewFolder) -> Result<FolderMeta> {
        let _g = self.op.lock();
        if f.title.trim().is_empty() {
            return Err(err(ErrorCode::InvalidArgument, "empty folder title"));
        }
        let dir = unique_path(&self.root, &sanitize_name(&f.title), "");
        let id = Ulid::new();
        let mut doc = FolderDoc::new(&id.to_string(), f.title.trim());
        doc.tags = f.tags;
        doc.allow_cloud = f.allow_cloud;
        doc.set_instructions(&f.instructions);
        fs::create_dir_all(dir.join(CALLS_SUBDIR))?;
        atomic_write(&dir.join(FOLDER_FILE), folder_codec::render(&doc).as_bytes())?;
        self.resync()?;
        let fid = FolderRef::User(id);
        self.folders()?
            .into_iter()
            .find(|m| m.id == fid)
            .ok_or_else(|| err(ErrorCode::Io, "created folder not found"))
    }

    pub fn folder_doc(&self, id: &Ulid) -> Result<FolderDoc> {
        self.find_folder(id)?
            .doc
            .ok_or_else(|| err(ErrorCode::NotFound, "no folder card"))
    }

    pub fn update_folder(&self, id: &Ulid, p: FolderPatch) -> Result<FolderMeta> {
        let _g = self.op.lock();
        let rec = self.find_folder(id)?;
        let mut doc = rec
            .doc
            .ok_or_else(|| err(ErrorCode::NotFound, "no folder card"))?;
        let mut dir = self.root.join(&rec.dir);
        if let Some(t) = p.title.filter(|t| !t.trim().is_empty() && *t != doc.title) {
            doc.title = t.trim().to_string();
            let want = sanitize_name(&doc.title);
            if want != rec.dir {
                let dst = unique_path(&self.root, &want, "");
                fs::rename(&dir, &dst)?;
                dir = dst;
            }
        }
        if let Some(t) = p.tags {
            doc.tags = t;
        }
        if let Some(c) = p.allow_cloud {
            doc.allow_cloud = c;
        }
        if let Some(t) = p.default_template {
            doc.default_template = t;
        }
        if let Some(m) = p.mic {
            doc.mic = m;
        }
        if let Some(m) = p.retx {
            doc.retx = m;
        }
        if let Some(i) = p.instructions {
            doc.set_instructions(&i);
        }
        atomic_write(&dir.join(FOLDER_FILE), folder_codec::render(&doc).as_bytes())?;
        self.resync()?;
        let fid = FolderRef::User(*id);
        self.folders()?
            .into_iter()
            .find(|m| m.id == fid)
            .ok_or_else(|| err(ErrorCode::NotFound, "folder"))
    }

    // ---- calls ----------------------------------------------------------

    pub fn calls(&self, folder: Option<&Ulid>) -> Result<Vec<CallMeta>> {
        match folder {
            Some(id) => self.calls_in(&FolderRef::User(*id)),
            None => Ok(self.scan_calls()?.into_iter().filter(|c| !c.deleted).collect()),
        }
    }

    /// Active calls plus the trashed ones from ONE scan (two scans could disagree about a call being written).
    /// `deleted` is true exactly for the calls in the trash.
    pub fn calls_with_trash(&self) -> Result<Vec<CallMeta>> {
        Ok(self
            .scan_calls()?
            .into_iter()
            .map(|mut c| {
                c.deleted = c.folder == FolderRef::Trash;
                c
            })
            .collect())
    }

    pub fn calls_in(&self, folder: &FolderRef) -> Result<Vec<CallMeta>> {
        Ok(self
            .scan_calls()?
            .into_iter()
            .filter(|c| c.folder == *folder)
            .collect())
    }

    fn scan_calls(&self) -> Result<Vec<CallMeta>> {
        let mut v: Vec<CallMeta> = self.scan()?.files.into_iter().filter_map(|f| f.meta).collect();
        v.sort_by(|a, b| b.started.cmp(&a.started).then(a.id.cmp(&b.id)));
        Ok(v)
    }

    pub fn read_call(&self, id: &Ulid) -> Result<Call> {
        let rec = self.find(id)?;
        let meta = rec.meta.ok_or_else(|| err(ErrorCode::NotFound, "not a call"))?;
        let md = fs::read_to_string(self.root.join(&rec.rel))?;
        Ok(Call { meta, md })
    }

    pub fn read_call_doc(&self, id: &Ulid) -> Result<CallDoc> {
        Ok(call_codec::parse(&self.read_call(id)?.md))
    }

    /// Replace the whole file (UI editor). Rejected while the call is recording.
    pub fn save_md(&self, id: &Ulid, md: &str) -> Result<()> {
        let _g = self.op.lock();
        if self.live.lock().contains(id) {
            return Err(err(ErrorCode::SessionBusy, "call is being recorded"));
        }
        let rec = self.find(id)?;
        if rec.deleted {
            return Err(err(ErrorCode::InvalidArgument, "call is in the trash"));
        }
        let doc = call_codec::parse(md);
        if doc.fm.id != id.to_string() {
            return Err(err(ErrorCode::FormatInvalid, "frontmatter id does not match"));
        }
        let path = self.root.join(&rec.rel);
        atomic_write(&path, fm::normalize_eol(md).as_bytes())?;
        self.rename_to_title(&path, &doc.fm)?;
        self.resync()
    }

    /// Mutate a call through the codec and write it back atomically.
    pub fn update_call(&self, id: &Ulid, f: impl FnOnce(&mut CallDoc)) -> Result<CallMeta> {
        let _g = self.op.lock();
        if self.live.lock().contains(id) {
            return Err(err(ErrorCode::SessionBusy, "call is being recorded"));
        }
        let rec = self.find(id)?;
        let path = self.root.join(&rec.rel);
        let mut doc = call_codec::parse(&fs::read_to_string(&path)?);
        f(&mut doc);
        atomic_write(&path, call_codec::render(&doc).as_bytes())?;
        let path = self.rename_to_title(&path, &doc.fm)?;
        self.resync()?;
        self.find(id).and_then(|r| {
            let _ = path;
            r.meta.ok_or_else(|| err(ErrorCode::NotFound, "call"))
        })
    }

    pub fn set_call_tags(&self, id: &Ulid, tags: Vec<String>) -> Result<CallMeta> {
        self.update_call(id, |d| d.fm.tags = tags)
    }

    pub fn set_call_summary(&self, id: &Ulid, summary: &str) -> Result<CallMeta> {
        self.update_call(id, |d| d.set_summary(summary))
    }

    /// Move a call to another folder (moving to `Trash` = `delete`).
    pub fn move_call(&self, id: &Ulid, to: &FolderRef) -> Result<CallMeta> {
        if *to == FolderRef::Trash {
            self.delete(id)?;
            return self
                .find(id)?
                .meta
                .ok_or_else(|| err(ErrorCode::NotFound, "call"));
        }
        let _g = self.op.lock();
        if self.live.lock().contains(id) {
            return Err(err(ErrorCode::SessionBusy, "call is being recorded"));
        }
        let rec = self.find(id)?;
        if rec.deleted {
            return Err(err(ErrorCode::InvalidArgument, "restore the call first"));
        }
        let dir = self.folder_dir(to)?;
        let title = match to {
            FolderRef::User(fid) => self.find_folder(fid)?.doc.map(|d| d.title).unwrap_or(dir.clone()),
            _ => dir.clone(),
        };
        let src = self.root.join(&rec.rel);
        let calls_dir = self.root.join(&dir).join(CALLS_SUBDIR);
        fs::create_dir_all(&calls_dir)?;
        let stem = src
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dst = unique_path(&calls_dir, &stem, "md");
        self.move_with_assets(&src, &dst)?;
        let mut doc = call_codec::parse(&fs::read_to_string(&dst)?);
        doc.fm.folder = title;
        atomic_write(&dst, call_codec::render(&doc).as_bytes())?;
        self.resync()?;
        self.find(id)?
            .meta
            .ok_or_else(|| err(ErrorCode::NotFound, "call"))
    }

    /// Rename `src.md` (+ `src.assets/`) to `dst.md`, rewriting asset embeds.
    fn move_with_assets(&self, src: &Path, dst: &Path) -> Result<()> {
        let stem = |p: &Path| {
            p.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let (os, ns) = (stem(src), stem(dst));
        fs::rename(src, dst)?;
        let old_assets = src.with_file_name(format!("{os}.assets"));
        if old_assets.is_dir() {
            fs::rename(&old_assets, dst.with_file_name(format!("{ns}.assets")))?;
        }
        if os != ns {
            if let Ok(t) = fs::read_to_string(dst) {
                let n = t.replace(&format!("![[{os}.assets/"), &format!("![[{ns}.assets/"));
                if n != t {
                    atomic_write(dst, n.as_bytes())?;
                }
            }
        }
        Ok(())
    }

    /// Title edits rename the file (`YYYY-MM-DD HHMM Title.md`).
    fn rename_to_title(&self, path: &Path, f: &CallFm) -> Result<PathBuf> {
        let want = format!("{} {}", file_stamp(&f.started), sanitize_name(&f.title));
        let cur = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let base = cur.split(" (").next().unwrap_or(&cur);
        if want == cur || want == base {
            return Ok(path.to_path_buf());
        }
        let dst = unique_path(path.parent().unwrap_or(&self.root), &want, "md");
        self.move_with_assets(path, &dst)?;
        Ok(dst)
    }

    // ---- trash ----------------------------------------------------------

    /// Moves to the app-level Trash (`.lowk/trash`); never hard-deletes.
    pub fn delete(&self, id: &Ulid) -> Result<()> {
        let _g = self.op.lock();
        if self.live.lock().contains(id) {
            return Err(err(ErrorCode::SessionBusy, "call is being recorded"));
        }
        let sc = self.scan()?;
        if let Some(f) = sc.files.iter().find(|f| f.id == *id && f.kind == ItemKind::Doc) {
            // documents move with their original and assets (docs.rs)
            return if f.deleted {
                Ok(())
            } else {
                self.trash_doc_locked(f)
            };
        }
        let tdir = self.trash_root().join(id.to_string());
        let (kind, title, orig, names, moves): (
            ItemKind,
            String,
            String,
            Vec<String>,
            Vec<(PathBuf, PathBuf)>,
        ) = if let Some(f) = sc.files.iter().find(|f| f.id == *id) {
            if f.deleted {
                return Ok(());
            }
            let src = self.root.join(&f.rel);
            let stem = src
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut names = vec![src
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()];
            let mut moves = vec![(src.clone(), tdir.join(&names[0]))];
            let assets = src.with_file_name(format!("{stem}.assets"));
            if assets.is_dir() {
                let n = format!("{stem}.assets");
                moves.push((assets, tdir.join(&n)));
                names.push(n);
            }
            (f.kind, f.title.clone(), f.rel.clone(), names, moves)
        } else if let Some(fo) = sc.folders.iter().find(|f| f.fref == FolderRef::User(*id)) {
            let t = fo.doc.as_ref().map(|d| d.title.clone()).unwrap_or_default();
            (
                ItemKind::Folder,
                t,
                fo.dir.clone(),
                vec![fo.dir.clone()],
                vec![(self.root.join(&fo.dir), tdir.join(&fo.dir))],
            )
        } else {
            return Err(err(ErrorCode::NotFound, format!("no item {id}")));
        };
        fs::create_dir_all(&tdir)?;
        for (s, d) in moves {
            fs::rename(s, d)?;
        }
        let item = TrashItem {
            id: *id,
            kind,
            title,
            orig_path: orig,
            deleted_at: fmt_rfc3339(::time::OffsetDateTime::now_utc()),
            names,
        };
        let json = serde_json::to_vec_pretty(&item).map_err(|e| err(ErrorCode::Io, e.to_string()))?;
        atomic_write(&self.trash_root().join(format!("{id}.trash.json")), &json)?;
        self.resync()
    }

    pub fn trash_items(&self) -> Result<Vec<TrashItem>> {
        let mut out = vec![];
        let Ok(rd) = fs::read_dir(self.trash_root()) else {
            return Ok(out);
        };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.ends_with(".trash.json") {
                if let Ok(item) = serde_json::from_slice::<TrashItem>(&fs::read(e.path())?) {
                    out.push(item);
                }
            }
        }
        out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    /// Put a trashed item back where it was (name collision -> numeric suffix).
    pub fn restore(&self, id: &Ulid) -> Result<()> {
        let _g = self.op.lock();
        let item = self
            .trash_items()?
            .into_iter()
            .find(|i| i.id == *id)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("{id} is not in the trash")))?;
        if item.kind == ItemKind::Doc {
            return self.restore_doc_locked(&item);
        }
        let tdir = self.trash_root().join(id.to_string());
        let orig = self.root.join(&item.orig_path);
        let parent = orig.parent().unwrap_or(&self.root).to_path_buf();
        fs::create_dir_all(&parent)?;
        match item.kind {
            ItemKind::Folder => {
                let dst = unique_path(&parent, &item.names[0], "");
                fs::rename(tdir.join(&item.names[0]), dst)?;
            }
            _ => {
                let md_name = &item.names[0];
                let stem = md_name.strip_suffix(".md").unwrap_or(md_name);
                let dst = unique_path(&parent, stem, "md");
                let nstem = dst
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                fs::rename(tdir.join(md_name), &dst)?;
                if let Some(a) = item.names.get(1) {
                    fs::rename(tdir.join(a), parent.join(format!("{nstem}.assets")))?;
                }
                if nstem != stem {
                    if let Ok(t) = fs::read_to_string(&dst) {
                        let n = t.replace(&format!("![[{stem}.assets/"), &format!("![[{nstem}.assets/"));
                        atomic_write(&dst, n.as_bytes())?;
                    }
                }
            }
        }
        let _ = fs::remove_dir_all(&tdir);
        fs::remove_file(self.trash_root().join(format!("{id}.trash.json")))?;
        self.resync()
    }

    /// The only hard delete: "Empty trash". Removes everything under `.lowk/trash`.
    pub fn empty_trash(&self) -> Result<usize> {
        let _g = self.op.lock();
        let items = self.trash_items()?;
        let n = items.len();
        // saved audio and transcript revisions go with their call (and with every call inside a trashed folder)
        for it in &items {
            self.purge_item_files(it);
        }
        let t = self.trash_root();
        if t.exists() {
            fs::remove_dir_all(&t)?;
        }
        fs::create_dir_all(&t)?;
        self.resync()?;
        Ok(n)
    }

    /// «Delete forever» for one trashed item: removes it from the trash together with its saved audio
    /// (`.lowk/audio/<call>/`) and transcript revisions (`.lowk/revisions/<call>/`) - for a folder, those of
    /// every call inside it. Moving to the trash keeps all of that so a restore is complete.
    pub fn delete_forever(&self, id: &Ulid) -> Result<()> {
        let _g = self.op.lock();
        let item = self
            .trash_items()?
            .into_iter()
            .find(|i| i.id == *id)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("{id} is not in the trash")))?;
        self.purge_item_files(&item);
        let _ = fs::remove_dir_all(self.trash_root().join(id.to_string()));
        fs::remove_file(self.trash_root().join(format!("{id}.trash.json")))?;
        self.resync()
    }

    /// Audio + revisions of a trashed item's call(s).
    fn purge_item_files(&self, it: &TrashItem) {
        let svc = self.service();
        let mut ids = vec![it.id];
        fn walk(dir: &Path, ids: &mut Vec<Ulid>) {
            let Ok(rd) = fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, ids);
                } else if p.extension().is_some_and(|x| x == "md") {
                    if let Ok(t) = fs::read_to_string(&p) {
                        let t = crate::fm::normalize_eol(&t);
                        if let (Some(front), _) = crate::fm::split(&t) {
                            if let Some(id) = front
                                .get("id")
                                .and_then(|v| crate::fm::scalar(v).parse::<Ulid>().ok())
                            {
                                ids.push(id);
                            }
                        }
                    }
                }
            }
        }
        if it.kind == ItemKind::Folder {
            walk(&self.trash_root().join(it.id.to_string()), &mut ids);
        }
        for id in ids {
            for d in ["audio", "revisions"] {
                let _ = fs::remove_dir_all(svc.join(d).join(id.to_string()));
            }
        }
    }

    // ---- documents ------------------------------------------------------

    /// Converted documents (`type: doc`) of a folder: `(id, title, path)`.
    pub fn folder_docs(&self, folder: &FolderRef) -> Result<Vec<(Ulid, String, String)>> {
        Ok(self
            .scan()?
            .files
            .into_iter()
            .filter(|f| f.kind == ItemKind::Doc && f.folder == *folder && !f.deleted)
            .map(|f| (f.id, f.title, f.rel))
            .collect())
    }

    pub(crate) fn rescan_meta(&self, id: &Ulid) -> Option<CallMeta> {
        self.find(id).ok().and_then(|r| r.meta)
    }

    pub(crate) fn scan_find_path(&self, id: &Ulid) -> Option<PathBuf> {
        self.find(id).ok().map(|r| self.root.join(r.rel))
    }

    pub(crate) fn op_lock(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.op.lock()
    }
}

pub(crate) fn wall_ms() -> i64 {
    (::time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}
