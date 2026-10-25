//! `Retriever` for Ask: folder instructions + pinned documents + relevant chunks of documents
//! and of previous calls of the same folder, packed into token budgets, with a `SourceRef` on
//! every chunk. The current call is excluded (it is the live delta), `qa` chunks are never
//! retrieved (no self-confirmation loop), and `allow_cloud = false` folders are withheld when
//! the provider is a cloud one.
use crate::db::{db_err, folder_key};
use crate::index::Index;
use crate::search::{map_row, Row};
use lowk_types::{
    ChunkKind, ContextChunk, ContextPack, FolderRef, RefKind, Result, RetrievalQuery, Retriever,
    SearchFilters, SearchMode, SourceRef,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

/// Pinned budget used when the query leaves it at 0.
pub const DEFAULT_PINNED_BUDGET: u32 = 6000;
/// The search text is the tail of the query: roughly the last 300 tokens.
const QUERY_TAIL_CHARS: usize = 900;
const SAME_FOLDER_BOOST: f32 = 1.3;

struct FolderInfo {
    title: String,
    allow_cloud: bool,
    instructions: String,
}

/// The query comes from a local-only folder (its call transcript): it must not be embedded in the cloud.
fn scope_local_only(q: &RetrievalQuery, folders: &HashMap<String, FolderInfo>) -> bool {
    q.scope.folder.is_some_and(|f| {
        folders
            .get(&folder_key(&FolderRef::User(f)))
            .is_some_and(|i| !i.allow_cloud)
    })
}

fn tail(text: &str) -> &str {
    let t = text.trim();
    let n = t.chars().count();
    if n <= QUERY_TAIL_CHARS {
        return t;
    }
    let skip = t
        .char_indices()
        .nth(n - QUERY_TAIL_CHARS)
        .map(|x| x.0)
        .unwrap_or(0);
    &t[skip..]
}

fn to_chunk(r: &Row, score: f32, pinned: bool) -> ContextChunk {
    let mut sr = r.source_ref();
    sr.pinned = pinned;
    ContextChunk {
        id: r.cid.clone(),
        kind: r.kind,
        text: r.text.clone(),
        source: r.title.clone(),
        heading_path: r.heading.clone(),
        page: r.page,
        score,
        source_ref: sr,
        pinned,
        tokens: r.n_tokens,
    }
}

impl Index {
    fn folder_infos(&self) -> Result<HashMap<String, FolderInfo>> {
        let r = self.inner.r.lock();
        let mut st = r
            .prepare_cached("SELECT key, title, allow_cloud, instructions FROM folders")
            .map_err(db_err)?;
        let it = st
            .query_map([], |x| {
                Ok((
                    x.get::<_, String>(0)?,
                    FolderInfo {
                        title: x.get(1)?,
                        allow_cloud: x.get::<_, i64>(2)? != 0,
                        instructions: x.get(3)?,
                    },
                ))
            })
            .map_err(db_err)?;
        it.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    fn relevant(
        &self,
        q: &RetrievalQuery,
        folders: &HashMap<String, FolderInfo>,
        skip: &HashSet<String>,
    ) -> Result<(Vec<ContextChunk>, u32)> {
        let scope_folder = q.scope.folder.map(|f| folder_key(&FolderRef::User(f)));
        let filters = SearchFilters {
            folders: match (&q.scope.folder, q.scope.all_folders) {
                (Some(f), false) => vec![FolderRef::User(*f)],
                _ => vec![],
            },
            ..SearchFilters::default()
        };
        let (mut cands, _) = self.inner.candidates(
            tail(&q.text),
            &filters,
            SearchMode::Hybrid,
            50,
            scope_local_only(q, folders),
        )?;
        let current = q.scope.call.map(|c| c.to_string());
        let exclude: HashSet<&str> = q.exclude.iter().map(String::as_str).collect();
        let mut withheld = 0u32;
        cands.retain(|c| {
            let r = &c.row;
            if r.kind == ChunkKind::Qa
                || Some(&r.file_id) == current.as_ref()
                || exclude.contains(r.cid.as_str())
                || skip.contains(&r.cid)
                || (!q.include_previous_calls && r.file_kind != "doc")
            {
                return false;
            }
            if q.cloud_provider && !folders.get(&r.folder).is_some_and(|f| f.allow_cloud) {
                withheld += 1;
                return false;
            }
            true
        });
        if q.scope.all_folders {
            for c in &mut cands {
                if Some(&c.row.folder) == scope_folder.as_ref() {
                    c.score *= SAME_FOLDER_BOOST;
                }
            }
            cands.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.row.id.cmp(&b.row.id))
            });
        }
        let mut used = 0u32;
        let mut out = vec![];
        let mut misses = 0;
        for c in &cands {
            if used + c.row.n_tokens > q.budget_tokens {
                misses += 1;
                if misses >= 4 {
                    break;
                }
                continue;
            }
            used += c.row.n_tokens;
            out.push(to_chunk(&c.row, c.score, false));
        }
        Ok((out, withheld))
    }

    fn pinned(
        &self,
        q: &RetrievalQuery,
        folders: &HashMap<String, FolderInfo>,
    ) -> Result<(Vec<ContextChunk>, u32, u32)> {
        let Some(folder) = q.scope.folder else {
            return Ok((vec![], 0, 0));
        };
        let key = folder_key(&FolderRef::User(folder));
        let rows: Vec<Row> = {
            let r = self.inner.r.lock();
            let sql = format!(
                "{} WHERE f.folder=?1 AND f.pinned=1 AND f.deleted=0 AND f.kind='doc' ORDER BY f.when_ms, f.fid, c.ord",
                "SELECT c.id, c.cid, c.fid, c.ord, c.kind, c.start_s, c.page, c.heading, c.text, c.n_tokens,
                 f.id, f.kind, f.folder, f.title, f.tags, f.when_ts, f.when_ms, f.pinned
                 FROM chunks c JOIN files f ON f.fid=c.fid"
            );
            let mut st = r.prepare(&sql).map_err(db_err)?;
            let it = st.query_map([&key], map_row).map_err(db_err)?;
            it.collect::<std::result::Result<_, _>>().map_err(db_err)?
        };
        if rows.is_empty() {
            return Ok((vec![], 0, 0));
        }
        if q.cloud_provider && !folders.get(&key).is_some_and(|f| f.allow_cloud) {
            return Ok((vec![], 0, rows.len() as u32));
        }
        let budget = if q.pinned_budget_tokens == 0 {
            DEFAULT_PINNED_BUDGET
        } else {
            q.pinned_budget_tokens
        };
        let mut by_doc: Vec<(i64, Vec<&Row>)> = vec![];
        for r in &rows {
            match by_doc.last_mut() {
                Some((fid, v)) if *fid == r.fid => v.push(r),
                _ => by_doc.push((r.fid, vec![r])),
            }
        }
        let exclude: HashSet<&str> = q.exclude.iter().map(String::as_str).collect();
        let mut used = 0u32;
        let mut out = vec![];
        let mut dropped = 0u32;
        for (_, chunks) in &by_doc {
            let tk: u32 = chunks.iter().map(|c| c.n_tokens).sum();
            // a pinned document goes in whole or not at all
            if used + tk > budget {
                dropped += 1;
                continue;
            }
            used += tk;
            out.extend(
                chunks
                    .iter()
                    .filter(|c| !exclude.contains(c.cid.as_str()))
                    .map(|c| to_chunk(c, 1.0, true)),
            );
        }
        Ok((out, dropped, 0))
    }

    fn instructions(
        &self,
        q: &RetrievalQuery,
        folders: &HashMap<String, FolderInfo>,
    ) -> (Option<ContextChunk>, u32) {
        let Some(folder) = q.scope.folder else {
            return (None, 0);
        };
        let key = folder_key(&FolderRef::User(folder));
        let Some(info) = folders.get(&key) else {
            return (None, 0);
        };
        if info.instructions.trim().is_empty() {
            return (None, 0);
        }
        if q.cloud_provider && !info.allow_cloud {
            return (None, 1);
        }
        let text = info.instructions.trim().to_string();
        let tokens = crate::text::estimate_tokens(&text);
        let sr = SourceRef {
            kind: RefKind::Instructions,
            id: folder,
            folder: FolderRef::User(folder),
            title: info.title.clone(),
            at_s: None,
            page: None,
            heading_path: vec![],
            chunk_id: None,
            pinned: false,
        };
        (
            Some(ContextChunk {
                id: format!("instr:{folder}"),
                kind: ChunkKind::Doc,
                text,
                source: info.title.clone(),
                heading_path: vec![],
                page: None,
                score: 1.0,
                source_ref: sr,
                pinned: false,
                tokens,
            }),
            0,
        )
    }

    /// Computes (and caches until the index changes) the pack for `q`, so the hotkey finds it
    /// ready. Call it when a chunk or a question closes during a call.
    pub fn warm_up(&self, q: &RetrievalQuery) -> Result<()> {
        let pack = self.build_pack(q)?;
        *self.inner.pack_cache.lock() = Some((
            self.inner.generation.load(Ordering::Relaxed),
            format!("{q:?}"),
            pack,
        ));
        Ok(())
    }

    fn build_pack(&self, q: &RetrievalQuery) -> Result<ContextPack> {
        let folders = self.folder_infos()?;
        let mut pack = ContextPack::default();
        let mut skip: HashSet<String> = HashSet::new();
        if q.include_folder_context {
            let (ins, w) = self.instructions(q, &folders);
            pack.instructions = ins;
            pack.withheld += w;
            let (pinned, dropped, w) = self.pinned(q, &folders)?;
            pack.pinned_dropped = dropped;
            pack.withheld += w;
            skip = pinned.iter().map(|c| c.id.clone()).collect();
            pack.pinned = pinned;
        }
        let (rel, w) = self.relevant(q, &folders, &skip)?;
        pack.relevant = rel;
        pack.withheld += w;
        Ok(pack)
    }
}

impl Retriever for Index {
    fn retrieve(&self, q: &RetrievalQuery) -> Result<Vec<ContextChunk>> {
        let folders = self.folder_infos()?;
        Ok(self.relevant(q, &folders, &HashSet::new())?.0)
    }

    fn retrieve_pack(&self, q: &RetrievalQuery) -> Result<ContextPack> {
        let gen = self.inner.generation.load(Ordering::Relaxed);
        let key = format!("{q:?}");
        if let Some((g, k, p)) = &*self.inner.pack_cache.lock() {
            if *g == gen && *k == key {
                return Ok(p.clone());
            }
        }
        self.build_pack(q)
    }
}
