//! Vault -> index. The index never reads the vault, so this feeds it `IndexItem`s: a full
//! reconcile at start (diff of `known()` against the vault by content hash, so deleting
//! `.lowk/index.db` and relaunching rebuilds the same index) and incremental upserts for every
//! store change. Documents move `Indexing -> Ready` here, once their chunks are in the index.
use super::{Job, Services};
use lowk_store::{fm, Vault};
use lowk_types::*;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Weak;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(250);

pub(crate) fn spawn(svc: Weak<Services>, rx: Receiver<Job>) {
    std::thread::Builder::new()
        .name("lowk-index-feed".into())
        .spawn(move || loop {
            let Ok(first) = rx.recv() else { return };
            let mut jobs = vec![first];
            loop {
                match rx.recv_timeout(DEBOUNCE) {
                    Ok(j) => {
                        jobs.push(j);
                        if jobs.len() > 20_000 {
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            let Some(svc) = svc.upgrade() else { return };
            svc.run_jobs(jobs);
        })
        .expect("spawn index feed");
}

fn folder_key(f: &FolderRef) -> String {
    match f {
        FolderRef::User(id) => id.to_string(),
        FolderRef::Unsorted => SYS_UNSORTED_ID.into(),
        FolderRef::Trash => SYS_TRASH_ID.into(),
    }
}

/// blake3 over everything the index stores about the item, so a rename, a pin or a tag change
/// re-indexes as well as a text change.
pub(crate) fn item_hash(i: &IndexItem) -> String {
    let mut h = blake3::Hasher::new();
    for part in [
        format!("{:?}", i.kind),
        i.title.clone(),
        i.tags.join("\u{1f}"),
        i.pinned.to_string(),
        i.deleted.to_string(),
        folder_key(&i.folder),
        i.when.clone().unwrap_or_default(),
    ] {
        h.update(part.as_bytes());
        h.update(&[0]);
    }
    h.update(i.md.as_bytes());
    h.finalize().to_hex().to_string()
}

fn finish(mut i: IndexItem) -> IndexItem {
    i.content_hash = item_hash(&i);
    i
}

pub(crate) fn call_item(vault: &Vault, id: &Ulid) -> Result<IndexItem> {
    let c = vault.read_call(id)?;
    let body = fm::split(&c.md).1.to_string();
    Ok(finish(IndexItem {
        id: c.meta.id,
        kind: IndexItemKind::Call,
        folder: c.meta.folder.clone(),
        title: c.meta.title.clone(),
        tags: c.meta.tags.clone(),
        when: Some(c.meta.started.clone()),
        md: body,
        content_hash: String::new(),
        pinned: false,
        deleted: c.meta.deleted,
    }))
}

/// A document is indexable once it has converted text (`Indexing` / `Ready`).
pub(crate) fn doc_item(vault: &Vault, d: &Document) -> Option<IndexItem> {
    if !matches!(d.status, IngestStatus::Indexing | IngestStatus::Ready) {
        return None;
    }
    let md = vault.read_doc_md(&d.id).ok()?;
    Some(finish(IndexItem {
        id: d.id,
        kind: IndexItemKind::Doc,
        folder: d.folder.clone(),
        title: d.title.clone(),
        tags: d.tags.clone(),
        when: Some(d.added.clone()),
        md,
        content_hash: String::new(),
        pinned: d.pinned,
        deleted: d.deleted,
    }))
}

pub(crate) fn index_folder(vault: &Vault, f: &FolderMeta) -> IndexFolder {
    let instructions = match &f.id {
        FolderRef::User(u) => vault
            .folder_doc(u)
            .map(|d| d.instructions().to_string())
            .unwrap_or_default(),
        _ => String::new(),
    };
    IndexFolder {
        folder: f.id.clone(),
        title: f.title.clone(),
        allow_cloud: f.allow_cloud,
        instructions,
        tags: f.tags.clone(),
    }
}

impl Services {
    pub(crate) fn run_jobs(&self, jobs: Vec<Job>) {
        if jobs.iter().any(|j| matches!(j, Job::Reconcile)) {
            if let Err(e) = self.reconcile() {
                tracing::warn!(?e, "index reconcile failed");
            }
        } else {
            let mut seen = HashSet::new();
            for j in jobs {
                let r = match j {
                    Job::Item(id) if seen.insert((false, id)) => self.index_one(&id),
                    Job::Folder(id) if seen.insert((true, id)) => self.index_folder_one(&id),
                    _ => continue,
                };
                if let Err(e) = r {
                    tracing::warn!(?e, "index update failed");
                }
            }
        }
        self.emit_status();
    }

    /// One call or document changed (or vanished).
    fn index_one(&self, id: &Ulid) -> Result<()> {
        use SearchIndex as _;
        if let Ok(d) = self.vault.get_doc(id) {
            if let Some(item) = doc_item(&self.vault, &d) {
                self.index.upsert(&item)?;
                self.doc_indexed(&d);
            }
            return Ok(());
        }
        match call_item(&self.vault, id) {
            Ok(item) => self.index.upsert(&item),
            Err(e) if e.code == ErrorCode::NotFound => self.index.remove(id, true),
            Err(e) => Err(e),
        }
    }

    fn index_folder_one(&self, id: &Ulid) -> Result<()> {
        use SearchIndex as _;
        if let Some(f) = self
            .vault
            .folders()?
            .into_iter()
            .find(|f| f.id == FolderRef::User(*id))
        {
            self.index.upsert_folder(&index_folder(&self.vault, &f))?;
        }
        Ok(())
    }

    /// `Indexing` -> `Ready`, announced to the UI.
    fn doc_indexed(&self, d: &Document) {
        if d.status != IngestStatus::Indexing {
            return;
        }
        match self.vault.set_doc_status(&d.id, IngestStatus::Ready, None) {
            Ok(doc) => self.env.emit(AppEvent::Ingest(IngestProgress {
                doc: doc.id,
                folder: doc.folder,
                status: IngestStatus::Ready,
                done: doc.pages.unwrap_or(0),
                total: doc.pages.unwrap_or(0),
                eta_s: None,
                error: None,
            })),
            Err(e) => tracing::warn!(?e, "could not mark the document ready"),
        }
    }

    /// Full diff of the vault against the index by content hash.
    pub fn reconcile(&self) -> Result<()> {
        use SearchIndex as _;
        let folders = self.vault.folders()?;
        for f in folders.iter().filter(|f| f.id != FolderRef::Trash) {
            self.index.upsert_folder(&index_folder(&self.vault, f))?;
        }
        let known: HashMap<Ulid, String> = self.index.known()?.into_iter().collect();
        let mut seen: HashSet<Ulid> = HashSet::new();
        let mut upserted = 0u32;
        let mut feed_item = |item: IndexItem, svc: &Services| -> Result<()> {
            seen.insert(item.id);
            if known.get(&item.id) != Some(&item.content_hash) {
                svc.index.upsert(&item)?;
                upserted += 1;
            }
            Ok(())
        };
        for f in &folders {
            for c in self.vault.calls_in(&f.id)? {
                match call_item(&self.vault, &c.id) {
                    Ok(item) => feed_item(item, self)?,
                    Err(e) => tracing::warn!(?e, id = %c.id, "call not indexed"),
                }
            }
            for d in self.vault.list_docs(&f.id)? {
                if let Some(item) = doc_item(&self.vault, &d) {
                    feed_item(item, self)?;
                    self.doc_indexed(&d);
                }
            }
        }
        let gone: Vec<Ulid> = known.keys().filter(|k| !seen.contains(k)).copied().collect();
        for id in &gone {
            self.index.remove(id, true)?;
        }
        tracing::info!(
            upserted,
            removed = gone.len(),
            known = known.len(),
            "index reconciled with the vault"
        );
        Ok(())
    }
}
