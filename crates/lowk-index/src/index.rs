//! The index: ingestion of `IndexItem`s, the embedding queue and status. Search lives in
//! `search.rs`, retrieval in `retrieve.rs`.
use crate::chunker::{chunk_markdown, CHUNKER_VER};
use crate::db::{self, db_err, folder_key};
use crate::embedder::SharedEmbedder;
use crate::text::stem_text;
use crate::vector::{self, FlatI8, VectorIndex};
use lowk_types::{
    AppError, ErrorCode, IndexFolder, IndexItem, IndexItemKind, IndexStatus, ReindexScope, Result,
    SearchIndex, SearchQuery, SearchResults, SemanticState, Ulid,
};
use parking_lot::{Condvar, Mutex, RwLock};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

pub type StatusFn = Box<dyn Fn(&IndexStatus) + Send + Sync>;

pub(crate) struct Inner {
    pub(crate) w: Mutex<Connection>,
    pub(crate) r: Mutex<Connection>,
    pub(crate) vecs: RwLock<FlatI8>,
    pub(crate) embedder: RwLock<Option<SharedEmbedder>>,
    pub(crate) override_state: Mutex<Option<SemanticState>>,
    pub(crate) generation: AtomicU64,
    on_status: Mutex<Option<StatusFn>>,
    paused: AtomicBool,
    wake: (Mutex<bool>, Condvar),
    pub(crate) last_error: Mutex<Option<AppError>>,
    pub(crate) ready_cache: Mutex<Option<(u64, bool)>>,
    /// RRF weights (FTS, vectors) and the cosine floor, as f32 bits.
    pub(crate) tuning: [std::sync::atomic::AtomicU32; 3],
    pub(crate) pack_cache: Mutex<Option<(u64, String, lowk_types::ContextPack)>>,
}

/// SQLite-backed hybrid index. Cheap to clone (shared state).
#[derive(Clone)]
pub struct Index {
    pub(crate) inner: Arc<Inner>,
}

impl Index {
    /// Opens (or creates) `index.db` at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let w = db::open(path)?;
        db::migrate(&w)?;
        let r = db::open(path)?;
        Ok(Self {
            inner: Arc::new(Inner {
                w: Mutex::new(w),
                r: Mutex::new(r),
                vecs: RwLock::new(FlatI8::new(0)),
                embedder: RwLock::new(None),
                override_state: Mutex::new(Some(SemanticState::Off)),
                generation: AtomicU64::new(0),
                on_status: Mutex::new(None),
                paused: AtomicBool::new(false),
                wake: (Mutex::new(false), Condvar::new()),
                last_error: Mutex::new(None),
                ready_cache: Mutex::new(None),
                tuning: [
                    std::sync::atomic::AtomicU32::new(crate::search::W_FTS.to_bits()),
                    std::sync::atomic::AtomicU32::new(crate::search::W_VEC.to_bits()),
                    std::sync::atomic::AtomicU32::new(crate::search::SEMANTIC_MIN_COSINE.to_bits()),
                ],
                pack_cache: Mutex::new(None),
            }),
        })
    }

    /// Called (from the embedding thread) when the status changes noticeably.
    pub fn on_status(&self, f: StatusFn) {
        *self.inner.on_status.lock() = Some(f);
    }

    /// Installs the embedder. Vectors of another model are dropped (re-embedded in the
    /// background); FTS keeps working meanwhile. `None` plus a state = `Off` / `ModelMissing` /
    /// `Failed`.
    pub fn set_embedder(&self, e: Option<SharedEmbedder>, state_without: SemanticState) -> Result<()> {
        let inner = &self.inner;
        match &e {
            Some(emb) => {
                let w = inner.w.lock();
                let prev = db::get_meta(&w, "embed_model");
                if prev.as_deref() != Some(emb.id()) {
                    w.execute("DELETE FROM vecs", []).map_err(db_err)?;
                    db::set_meta(&w, "embed_model", emb.id())?;
                }
                drop(w);
                let mut flat = FlatI8::new(emb.dim());
                let r = inner.r.lock();
                let mut st = r
                    .prepare(
                        "SELECT v.id, c.fid, v.vec FROM vecs v JOIN chunks c ON c.id=v.id WHERE v.model=?1",
                    )
                    .map_err(db_err)?;
                let rows = st
                    .query_map([emb.id()], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                        ))
                    })
                    .map_err(db_err)?;
                for row in rows {
                    let (id, fid, blob) = row.map_err(db_err)?;
                    flat.upsert(id, fid, &blob);
                }
                drop(st);
                drop(r);
                *inner.vecs.write() = flat;
                *inner.override_state.lock() = None;
            }
            None => {
                *inner.vecs.write() = FlatI8::new(0);
                *inner.override_state.lock() = Some(state_without);
            }
        }
        *inner.embedder.write() = e;
        inner.generation.fetch_add(1, Ordering::Relaxed);
        self.notify();
        Ok(())
    }

    /// Pauses background embedding (live STT on weak hardware).
    pub fn pause_embedding(&self, p: bool) {
        self.inner.paused.store(p, Ordering::Relaxed);
        if !p {
            self.notify();
        }
    }

    fn notify(&self) {
        *self.inner.wake.0.lock() = true;
        self.inner.wake.1.notify_all();
    }

    /// Starts the low-priority embedding thread; it ends when the last `Index` clone is dropped.
    pub fn start_background(&self) {
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        std::thread::Builder::new()
            .name("lowk-index-embed".into())
            .spawn(move || background(weak))
            .ok();
    }

    /// Embeds up to `max` pending chunks synchronously; returns how many were embedded.
    /// (The background thread calls this in small batches; tests and benchmarks call it directly.)
    pub fn embed_pending(&self, max: usize) -> Result<usize> {
        self.inner.embed_batch(max, &mut 0)
    }

    /// Chunks waiting for a vector (0 when there is no embedder).
    pub fn pending_vectors(&self) -> u32 {
        self.inner.counts().map(|c| c.pending_vecs).unwrap_or(0)
    }

    /// Evaluation knob: RRF weights of the keyword and vector lists and the cosine floor.
    pub fn set_tuning(&self, w_fts: f32, w_vec: f32, min_cosine: f32) {
        for (a, v) in self.inner.tuning.iter().zip([w_fts, w_vec, min_cosine]) {
            a.store(v.to_bits(), Ordering::Relaxed);
        }
        self.invalidate_caches();
    }

    /// Bumps the generation so cached retrieval packs are not reused.
    pub fn invalidate_caches(&self) {
        self.inner.generation.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.wake.1.notify_all();
    }
}

fn background(weak: Weak<Inner>) {
    let mut last_emit = Instant::now() - Duration::from_secs(10);
    let mut cursor = 0i64;
    loop {
        let Some(inner) = weak.upgrade() else { return };
        let idle = inner.paused.load(Ordering::Relaxed) || inner.embedder.read().is_none();
        let mut n = 0;
        if !idle {
            match inner.embed_batch(16, &mut cursor) {
                Ok(k) => n = k,
                Err(e) => {
                    tracing::warn!(code = ?e.code, "background embedding failed");
                    *inner.last_error.lock() = Some(e.clone());
                    *inner.override_state.lock() = Some(SemanticState::Failed { error: e });
                    // the embedder stays installed; a later set_embedder clears the failure
                    inner.paused.store(true, Ordering::Relaxed);
                }
            }
            if n == 0 {
                cursor = 0;
            }
        }
        let emit = n > 0 && last_emit.elapsed() > Duration::from_millis(700)
            || (n == 0 && !idle && last_emit.elapsed() > Duration::from_millis(50) && cursor == 0);
        if emit {
            last_emit = Instant::now();
            inner.emit();
        }
        if n == 0 {
            let mut g = inner.wake.0.lock();
            if !*g {
                inner.wake.1.wait_for(&mut g, Duration::from_millis(1500));
            }
            *g = false;
        }
        drop(inner);
    }
}

pub(crate) struct Counts {
    pub files: u32,
    pub chunks: u32,
    pub pending_files: u32,
    pub pending_vecs: u32,
    pub total_vecs_needed: u32,
}

impl Inner {
    pub(crate) fn counts(&self) -> Result<Counts> {
        let r = self.r.lock();
        let q = |sql: &str| -> Result<u32> {
            r.query_row(sql, [], |x| x.get::<_, i64>(0))
                .map(|v| v as u32)
                .map_err(db_err)
        };
        let files = q("SELECT COUNT(*) FROM files")?;
        let chunks = q("SELECT COUNT(*) FROM chunks")?;
        let pending_files = q("SELECT COUNT(*) FROM files WHERE content_hash=''")?;
        let (pending_vecs, total) = match self.embedder.read().as_ref() {
            Some(e) => {
                let policy = cloud_policy_sql(e.is_cloud());
                let total: u32 = r
                    .query_row(
                        &format!("SELECT COUNT(*) FROM chunks c JOIN files f ON f.fid=c.fid WHERE f.deleted=0{policy}"),
                        [],
                        |x| x.get::<_, i64>(0),
                    )
                    .map_err(db_err)? as u32;
                let have: u32 = r
                    .query_row(
                        &format!("SELECT COUNT(*) FROM vecs v JOIN chunks c ON c.id=v.id JOIN files f ON f.fid=c.fid WHERE v.model=?1 AND f.deleted=0{policy}"),
                        [e.id()],
                        |x| x.get::<_, i64>(0),
                    )
                    .map_err(db_err)? as u32;
                (total.saturating_sub(have), total)
            }
            None => (0, 0),
        };
        Ok(Counts {
            files,
            chunks,
            pending_files,
            pending_vecs,
            total_vecs_needed: total,
        })
    }

    pub(crate) fn status(&self) -> IndexStatus {
        let c = self.counts();
        let (files, chunks, pending, pv, total) = match &c {
            Ok(c) => (
                c.files,
                c.chunks,
                c.pending_files,
                c.pending_vecs,
                c.total_vecs_needed,
            ),
            Err(_) => (0, 0, 0, 0, 0),
        };
        let emb = self.embedder.read().clone();
        let semantic = match (&*self.override_state.lock(), &emb) {
            (Some(s), _) => s.clone(),
            (None, None) => SemanticState::Off,
            (None, Some(_)) if pv == 0 => SemanticState::Ready,
            (None, Some(_)) => SemanticState::Building {
                done: total.saturating_sub(pv),
                total,
            },
        };
        IndexStatus {
            files,
            chunks,
            pending,
            semantic,
            embedder_model: emb.map(|e| e.id().to_string()),
            last_error: self.last_error.lock().clone().or(c.err()),
        }
    }

    fn emit(&self) {
        if let Some(f) = &*self.on_status.lock() {
            f(&self.status());
        }
    }

    /// Embeds the next `max` chunks after `cursor` (rowid order) that lack a vector.
    fn embed_batch(&self, max: usize, cursor: &mut i64) -> Result<usize> {
        let Some(emb) = self.embedder.read().clone() else {
            return Ok(0);
        };
        let rows: Vec<(i64, i64, String, String, String)> = {
            let r = self.r.lock();
            let sql = format!(
                "SELECT c.id, c.fid, c.text, f.title, c.heading FROM chunks c
                 JOIN files f ON f.fid=c.fid
                 LEFT JOIN vecs v ON v.id=c.id AND v.model=?1
                 WHERE v.id IS NULL AND f.deleted=0 AND c.id>?2{}
                 ORDER BY c.id LIMIT ?3",
                cloud_policy_sql(emb.is_cloud())
            );
            let mut st = r.prepare(&sql).map_err(db_err)?;
            let it = st
                .query_map(params![emb.id(), *cursor, max as i64], |x| {
                    Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?, x.get(4)?))
                })
                .map_err(db_err)?;
            it.collect::<std::result::Result<_, _>>().map_err(db_err)?
        };
        if rows.is_empty() {
            return Ok(0);
        }
        let inputs: Vec<String> = rows
            .iter()
            .map(|(_, _, text, title, heading)| {
                let hp: Vec<String> = serde_json::from_str(heading).unwrap_or_default();
                let t = if hp.is_empty() {
                    title.clone()
                } else {
                    format!("{title} > {}", hp.join(" > "))
                };
                let body: String = text.chars().take(2400).collect();
                emb.format_doc(&t, &body)
            })
            .collect();
        let vecs = emb.embed_docs(&inputs)?;
        if vecs.len() != rows.len() {
            return Err(AppError::new(ErrorCode::EmbedFailed).with_detail("batch size mismatch"));
        }
        let mut w = self.w.lock();
        let tx = w.transaction().map_err(db_err)?;
        let mut flat = self.vecs.write();
        // The embedder may have been swapped while we were encoding; discard in that case.
        if self.embedder.read().as_ref().map(|e| e.id()) != Some(emb.id()) {
            return Ok(0);
        }
        for ((id, fid, ..), v) in rows.iter().zip(vecs) {
            let blob = vector::encode(&v);
            tx.execute(
                "INSERT INTO vecs(id,model,vec) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET model=excluded.model, vec=excluded.vec",
                params![id, emb.id(), blob],
            )
            .map_err(db_err)?;
            flat.upsert(*id, *fid, &blob);
        }
        tx.commit().map_err(db_err)?;
        self.generation.fetch_add(1, Ordering::Relaxed);
        *cursor = rows.last().map(|r| r.0).unwrap_or(*cursor);
        Ok(rows.len())
    }
}

/// Local-only policy: a cloud embedder never sees chunks of folders with `allow_cloud = 0`
/// (nor of files whose folder is unknown to the index).
pub(crate) fn cloud_policy_sql(cloud: bool) -> &'static str {
    if cloud {
        " AND EXISTS (SELECT 1 FROM folders fo WHERE fo.key=f.folder AND fo.allow_cloud=1)"
    } else {
        ""
    }
}

/// RFC 3339 or plain `YYYY-MM-DD` to epoch milliseconds.
pub(crate) fn when_ms(s: &str) -> Option<i64> {
    use time::format_description::well_known::Rfc3339;
    use time::{Date, Month, OffsetDateTime};
    if let Ok(t) = OffsetDateTime::parse(s, &Rfc3339) {
        return Some(t.unix_timestamp() * 1000);
    }
    let mut it = s.trim().splitn(3, '-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u8 = it.next()?.parse().ok()?;
    let d: u8 = it.next()?.parse().ok()?;
    let date = Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()?;
    Some(date.midnight().assume_utc().unix_timestamp() * 1000)
}

impl SearchIndex for Index {
    fn search(&self, q: &SearchQuery) -> Result<SearchResults> {
        self.inner.search(q)
    }

    fn status(&self) -> IndexStatus {
        self.inner.status()
    }

    fn reindex(&self, scope: ReindexScope) -> Result<()> {
        let w = self.inner.w.lock();
        match scope {
            // Hashes are blanked, so `known()` makes the app re-feed; the old rows keep serving
            // search until the new ones replace them (same chunk ids -> no churn).
            ReindexScope::All => {
                w.execute("UPDATE files SET content_hash=''", [])
                    .map_err(db_err)?;
                w.execute("DELETE FROM vecs", []).map_err(db_err)?;
                self.inner.vecs.write().clear();
            }
            ReindexScope::Item(id) => {
                w.execute(
                    "UPDATE files SET content_hash='' WHERE id=?1 OR folder=?1",
                    [id.to_string()],
                )
                .map_err(db_err)?;
                w.execute(
                    "DELETE FROM vecs WHERE id IN (SELECT c.id FROM chunks c JOIN files f ON f.fid=c.fid WHERE f.id=?1 OR f.folder=?1)",
                    [id.to_string()],
                )
                .map_err(db_err)?;
                self.inner.vecs.write().clear();
                // simplest consistent state: reload this model's vectors lazily
            }
        }
        drop(w);
        if let ReindexScope::Item(_) = scope {
            self.reload_vectors()?;
        }
        self.inner.generation.fetch_add(1, Ordering::Relaxed);
        self.notify();
        Ok(())
    }

    fn upsert(&self, item: &IndexItem) -> Result<()> {
        let id = item.id.to_string();
        let kind = match item.kind {
            IndexItemKind::Call => "call",
            IndexItemKind::Doc => "doc",
        };
        let folder = folder_key(&item.folder);
        let tags: Vec<String> = item
            .tags
            .iter()
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        let tags_json = serde_json::to_string(&item.tags).unwrap_or_else(|_| "[]".into());
        let when = item.when.as_deref().and_then(when_ms);
        let mut w = self.inner.w.lock();
        let tx = w.transaction().map_err(db_err)?;
        let existing: Option<(i64, String, i64)> = tx
            .query_row(
                "SELECT fid, content_hash, chunker_ver FROM files WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db_err)?;
        let fid = match &existing {
            Some((fid, ..)) => {
                tx.execute(
                    "UPDATE files SET kind=?2, folder=?3, title=?4, tags=?5, when_ts=?6, when_ms=?7, pinned=?8, deleted=?9 WHERE fid=?1",
                    params![fid, kind, folder, item.title, tags_json, item.when, when, item.pinned, item.deleted],
                )
                .map_err(db_err)?;
                *fid
            }
            None => {
                tx.execute(
                    "INSERT INTO files(id,kind,folder,title,tags,when_ts,when_ms,content_hash,pinned,deleted,chunker_ver)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,'',?8,?9,?10)",
                    params![id, kind, folder, item.title, tags_json, item.when, when, item.pinned, item.deleted, CHUNKER_VER],
                )
                .map_err(db_err)?;
                tx.last_insert_rowid()
            }
        };
        tx.execute("DELETE FROM file_tags WHERE fid=?1", [fid])
            .map_err(db_err)?;
        for t in &tags {
            tx.execute(
                "INSERT OR IGNORE INTO file_tags(fid,tag) VALUES(?1,?2)",
                params![fid, t],
            )
            .map_err(db_err)?;
        }
        let unchanged = existing.as_ref().is_some_and(|(_, h, v)| {
            *h == item.content_hash && *v == i64::from(CHUNKER_VER) && !h.is_empty()
        });
        let mut dropped: Vec<i64> = vec![];
        if !unchanged {
            let new_chunks = chunk_markdown(&id, item.kind, &item.md);
            let old: HashMap<String, i64> = {
                let mut st = tx
                    .prepare("SELECT cid, id FROM chunks WHERE fid=?1")
                    .map_err(db_err)?;
                let rows = st
                    .query_map([fid], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                    .map_err(db_err)?;
                rows.collect::<std::result::Result<_, _>>().map_err(db_err)?
            };
            let keep: HashSet<&str> = new_chunks.iter().map(|c| c.cid.as_str()).collect();
            for (cid, rid) in &old {
                if !keep.contains(cid.as_str()) {
                    tx.execute("DELETE FROM chunks WHERE id=?1", [rid])
                        .map_err(db_err)?;
                    tx.execute("DELETE FROM chunks_fts WHERE rowid=?1", [rid])
                        .map_err(db_err)?;
                    tx.execute("DELETE FROM vecs WHERE id=?1", [rid])
                        .map_err(db_err)?;
                    dropped.push(*rid);
                }
            }
            for c in &new_chunks {
                let heading = serde_json::to_string(&c.heading_path).unwrap_or_else(|_| "[]".into());
                let kind = chunk_kind_str(c.kind);
                if let Some(rid) = old.get(&c.cid) {
                    tx.execute(
                        "UPDATE chunks SET ord=?2, kind=?3, start_s=?4, page=?5, heading=?6, n_tokens=?7 WHERE id=?1",
                        params![rid, c.ord, kind, c.start_s, c.page, heading, c.n_tokens],
                    )
                    .map_err(db_err)?;
                } else {
                    tx.execute(
                        "INSERT INTO chunks(cid,fid,ord,kind,start_s,page,heading,text,n_tokens) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                        params![c.cid, fid, c.ord, kind, c.start_s, c.page, heading, c.text, c.n_tokens],
                    )
                    .map_err(db_err)?;
                    let rid = tx.last_insert_rowid();
                    tx.execute(
                        "INSERT INTO chunks_fts(rowid, stem) VALUES(?1, ?2)",
                        params![rid, stem_text(&c.index_text)],
                    )
                    .map_err(db_err)?;
                }
            }
            tx.execute(
                "UPDATE files SET content_hash=?2, chunker_ver=?3 WHERE fid=?1",
                params![fid, item.content_hash, CHUNKER_VER],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        drop(w);
        if !dropped.is_empty() {
            let mut v = self.inner.vecs.write();
            for d in dropped {
                v.remove(d);
            }
        }
        self.inner.generation.fetch_add(1, Ordering::Relaxed);
        self.notify();
        Ok(())
    }

    fn upsert_folder(&self, f: &IndexFolder) -> Result<()> {
        let w = self.inner.w.lock();
        w.execute(
            "INSERT INTO folders(key,title,allow_cloud,instructions,tags) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(key) DO UPDATE SET title=excluded.title, allow_cloud=excluded.allow_cloud,
               instructions=excluded.instructions, tags=excluded.tags",
            params![
                folder_key(&f.folder),
                f.title,
                f.allow_cloud,
                f.instructions,
                serde_json::to_string(&f.tags).unwrap_or_else(|_| "[]".into())
            ],
        )
        .map_err(db_err)?;
        self.inner.generation.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn known(&self) -> Result<Vec<(Ulid, String)>> {
        let r = self.inner.r.lock();
        let mut st = r.prepare("SELECT id, content_hash FROM files").map_err(db_err)?;
        let rows = st
            .query_map([], |x| Ok((x.get::<_, String>(0)?, x.get::<_, String>(1)?)))
            .map_err(db_err)?;
        let mut out = vec![];
        for row in rows {
            let (id, h) = row.map_err(db_err)?;
            if let Ok(u) = id.parse::<Ulid>() {
                out.push((u, h));
            }
        }
        Ok(out)
    }

    fn remove(&self, id: &Ulid, purge: bool) -> Result<()> {
        let w = self.inner.w.lock();
        let ids = id.to_string();
        let fid: Option<i64> = w
            .query_row("SELECT fid FROM files WHERE id=?1", [&ids], |r| r.get(0))
            .optional()
            .map_err(db_err)?;
        let Some(fid) = fid else { return Ok(()) };
        if !purge {
            w.execute("UPDATE files SET deleted=1 WHERE fid=?1", [fid])
                .map_err(db_err)?;
        } else {
            let rids: Vec<i64> = {
                let mut st = w.prepare("SELECT id FROM chunks WHERE fid=?1").map_err(db_err)?;
                let rows = st.query_map([fid], |r| r.get(0)).map_err(db_err)?;
                rows.collect::<std::result::Result<_, _>>().map_err(db_err)?
            };
            for rid in &rids {
                w.execute("DELETE FROM chunks_fts WHERE rowid=?1", [rid])
                    .map_err(db_err)?;
                w.execute("DELETE FROM vecs WHERE id=?1", [rid]).map_err(db_err)?;
            }
            w.execute("DELETE FROM chunks WHERE fid=?1", [fid])
                .map_err(db_err)?;
            w.execute("DELETE FROM file_tags WHERE fid=?1", [fid])
                .map_err(db_err)?;
            w.execute("DELETE FROM files WHERE fid=?1", [fid])
                .map_err(db_err)?;
            let mut v = self.inner.vecs.write();
            for rid in rids {
                v.remove(rid);
            }
        }
        self.inner.generation.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

impl Index {
    /// Starts the local query embedder (worker process, model load) with a constant string, so the
    /// first Ask does not pay for it. Never touches a cloud embedder; sends no user data.
    pub fn warm_query_embedder(&self) {
        let Some(emb) = self.inner.embedder.read().clone() else {
            return;
        };
        if emb.is_cloud() {
            return;
        }
        let t0 = std::time::Instant::now();
        let r = emb.embed_query(&emb.format_query("warm"));
        tracing::info!(
            ms = t0.elapsed().as_millis() as u64,
            ok = r.is_ok(),
            "query embedder warm"
        );
    }

    fn reload_vectors(&self) -> Result<()> {
        let emb = self.inner.embedder.read().clone();
        let Some(emb) = emb else { return Ok(()) };
        let mut flat = FlatI8::new(emb.dim());
        let r = self.inner.r.lock();
        let mut st = r
            .prepare("SELECT v.id, c.fid, v.vec FROM vecs v JOIN chunks c ON c.id=v.id WHERE v.model=?1")
            .map_err(db_err)?;
        let rows = st
            .query_map([emb.id()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(db_err)?;
        for row in rows {
            let (id, fid, blob) = row.map_err(db_err)?;
            flat.upsert(id, fid, &blob);
        }
        drop(st);
        drop(r);
        *self.inner.vecs.write() = flat;
        Ok(())
    }
}

pub(crate) fn chunk_kind_str(k: lowk_types::ChunkKind) -> &'static str {
    match k {
        lowk_types::ChunkKind::Speech => "speech",
        lowk_types::ChunkKind::Doc => "doc",
        lowk_types::ChunkKind::Qa => "qa",
        lowk_types::ChunkKind::Image => "image",
    }
}

pub(crate) fn parse_chunk_kind(s: &str) -> lowk_types::ChunkKind {
    match s {
        "speech" => lowk_types::ChunkKind::Speech,
        "qa" => lowk_types::ChunkKind::Qa,
        "image" => lowk_types::ChunkKind::Image,
        _ => lowk_types::ChunkKind::Doc,
    }
}
