//! Hybrid search: FTS top-50 + vector top-50, RRF (k = 60), boosts, filters, neighbour merge,
//! facets, snippets with UTF-16 highlights.
use crate::db::{db_err, folder_key, parse_folder_key};
use crate::index::{chunk_kind_str, parse_chunk_kind, when_ms, Inner};
use crate::text::{query_stems, utf16_offset, words};
use crate::vector::{normalize, VectorIndex};
use lowk_types::{
    ChunkKind, FacetCount, FolderRef, RefKind, Result, SearchFilters, SearchHit, SearchMode, SearchQuery,
    SearchResults, SourceRef, Ulid,
};
use rusqlite::params_from_iter;
use rusqlite::types::Value;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::time::Instant;

pub const RRF_K: f32 = 60.0;
pub const TOP_EACH: usize = 50;
/// A stem is never dropped as "very common" while it occurs in at most this many chunks.
const COMMON_STEM_FLOOR: i64 = 50;
/// Vector hits below this cosine are noise (set from the eval).
pub const SEMANTIC_MIN_COSINE: f32 = 0.0;
/// RRF list weights (tuned on the eval).
pub const W_FTS: f32 = 0.7;
pub const W_VEC: f32 = 1.0;
const QA_WEIGHT: f32 = 0.6;

#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub id: i64,
    pub cid: String,
    pub fid: i64,
    pub ord: i64,
    pub kind: ChunkKind,
    pub start_s: Option<u32>,
    pub page: Option<u32>,
    pub heading: Vec<String>,
    pub text: String,
    pub n_tokens: u32,
    pub file_id: String,
    pub file_kind: String,
    pub folder: String,
    pub title: String,
    pub tags: Vec<String>,
    pub when_ts: Option<String>,
    pub when_ms: Option<i64>,
    pub pinned: bool,
}

impl Row {
    pub(crate) fn source_ref(&self) -> SourceRef {
        let kind = match (self.file_kind.as_str(), self.kind) {
            (_, ChunkKind::Image) => RefKind::Image,
            ("doc", _) => RefKind::Doc,
            _ => RefKind::Call,
        };
        SourceRef {
            kind,
            id: Ulid::from_str(&self.file_id).unwrap_or_default(),
            folder: parse_folder_key(&self.folder),
            title: self.title.clone(),
            at_s: self.start_s,
            page: self.page,
            heading_path: self.heading.clone(),
            chunk_id: Some(self.cid.clone()),
            pinned: self.pinned,
        }
    }
}

const ROW_SQL: &str =
    "SELECT c.id, c.cid, c.fid, c.ord, c.kind, c.start_s, c.page, c.heading, c.text, c.n_tokens,
    f.id, f.kind, f.folder, f.title, f.tags, f.when_ts, f.when_ms, f.pinned
    FROM chunks c JOIN files f ON f.fid=c.fid";

pub(crate) fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    let heading: String = r.get(7)?;
    let tags: String = r.get(14)?;
    let kind: String = r.get(4)?;
    Ok(Row {
        id: r.get(0)?,
        cid: r.get(1)?,
        fid: r.get(2)?,
        ord: r.get(3)?,
        kind: parse_chunk_kind(&kind),
        start_s: r.get(5)?,
        page: r.get(6)?,
        heading: serde_json::from_str(&heading).unwrap_or_default(),
        text: r.get(8)?,
        n_tokens: r.get::<_, i64>(9)? as u32,
        file_id: r.get(10)?,
        file_kind: r.get(11)?,
        folder: r.get(12)?,
        title: r.get(13)?,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        when_ts: r.get(15)?,
        when_ms: r.get(16)?,
        pinned: r.get::<_, i64>(17)? != 0,
    })
}

/// SQL conditions over alias `f` (files) and `c` (chunks) plus their parameters.
fn filter_sql(f: &SearchFilters, skip_kinds: bool) -> (String, Vec<Value>) {
    let mut sql = String::new();
    let mut p: Vec<Value> = vec![];
    if !f.include_trash {
        sql.push_str(" AND f.deleted=0 AND f.folder<>?");
        p.push(Value::Text(lowk_types::SYS_TRASH_ID.into()));
    }
    if !f.folders.is_empty() {
        sql.push_str(&format!(
            " AND f.folder IN ({})",
            vec!["?"; f.folders.len()].join(",")
        ));
        p.extend(f.folders.iter().map(|x| Value::Text(folder_key(x))));
    }
    for t in &f.tags {
        sql.push_str(" AND EXISTS(SELECT 1 FROM file_tags t WHERE t.fid=f.fid AND t.tag=?)");
        p.push(Value::Text(t.trim().to_lowercase()));
    }
    if let Some(from) = f.from.as_deref().and_then(when_ms) {
        sql.push_str(" AND f.when_ms>=?");
        p.push(Value::Integer(from));
    }
    if let Some(to) = f.to.as_deref().and_then(when_ms) {
        // a plain date means the whole day
        let end = if f.to.as_deref().is_some_and(|s| s.len() <= 10) {
            to + 86_400_000 - 1
        } else {
            to
        };
        sql.push_str(" AND f.when_ms<=?");
        p.push(Value::Integer(end));
    }
    if !skip_kinds && !f.kinds.is_empty() {
        sql.push_str(&format!(
            " AND c.kind IN ({})",
            vec!["?"; f.kinds.len()].join(",")
        ));
        p.extend(f.kinds.iter().map(|k| Value::Text(chunk_kind_str(*k).into())));
    }
    (sql, p)
}

/// Share of the query stems present in each chunk (`ids` order).
fn coverage(r: &rusqlite::Connection, ids: &[i64], stems: &[String]) -> Result<Vec<f32>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let mut st = r
        .prepare(&format!(
            "SELECT rowid, stem FROM chunks_fts WHERE rowid IN ({list})"
        ))
        .map_err(db_err)?;
    let rows = st
        .query_map([], |x| Ok((x.get::<_, i64>(0)?, x.get::<_, String>(1)?)))
        .map_err(db_err)?;
    let mut by: HashMap<i64, f32> = HashMap::new();
    for row in rows {
        let (id, text) = row.map_err(db_err)?;
        let have: HashSet<&str> = text.split(' ').collect();
        let n = stems.iter().filter(|s| have.contains(s.as_str())).count();
        by.insert(id, n as f32 / stems.len().max(1) as f32);
    }
    Ok(ids.iter().map(|i| by.get(i).copied().unwrap_or(1.0)).collect())
}

pub(crate) struct Cand {
    pub row: Row,
    pub score: f32,
}

impl Inner {
    pub(crate) fn semantic_ready(&self) -> bool {
        if self.embedder.read().is_none() || self.override_state.lock().is_some() {
            return false;
        }
        let g = self.generation.load(std::sync::atomic::Ordering::Relaxed);
        let mut cache = self.ready_cache.lock();
        if let Some((cg, ready)) = *cache {
            if cg == g {
                return ready;
            }
        }
        let ready = self
            .counts()
            .map(|c| c.pending_vecs == 0 && c.total_vecs_needed > 0)
            .unwrap_or(false);
        *cache = Some((g, ready));
        ready
    }

    /// Candidate chunks with fused scores (boosts applied), best first, and the mode used.
    pub(crate) fn candidates(
        &self,
        text: &str,
        filters: &SearchFilters,
        mode: SearchMode,
        per_list: usize,
        no_cloud_query: bool,
    ) -> Result<(Vec<Cand>, SearchMode)> {
        let stems = query_stems(text);
        if stems.is_empty() {
            return Ok((vec![], mode));
        }
        // Local-only policy: the query of a local-only folder never goes to a cloud embedder.
        let blocked = no_cloud_query && self.embedder.read().as_ref().is_some_and(|e| e.is_cloud());
        let use_vec = mode != SearchMode::Keyword && !blocked && self.semantic_ready();
        let used = if use_vec { mode } else { SearchMode::Keyword };
        let t = |i: usize| f32::from_bits(self.tuning[i].load(std::sync::atomic::Ordering::Relaxed));
        let (w_fts, w_vec, min_cos) = (t(0), t(1), t(2));
        let r = self.r.lock();
        let mut fused: HashMap<i64, f32> = HashMap::new();
        if used != SearchMode::Semantic {
            let (cond, params) = filter_sql(filters, false);
            let ids = self.fts_ranked(&r, &stems, &cond, &params, per_list)?;
            let cov = coverage(&r, &ids, &stems)?;
            for (rank, (id, c)) in ids.into_iter().zip(cov).enumerate() {
                // a hit that matches only one of many query words is mostly noise: weight by the
                // squared share of query words it contains
                *fused.entry(id).or_default() += w_fts * c * c / (RRF_K + rank as f32 + 1.0);
            }
        }
        if use_vec {
            let emb = self.embedder.read().clone();
            if let Some(emb) = emb {
                match emb.embed_query(&emb.format_query(text)) {
                    Ok(mut q) => {
                        normalize(&mut q);
                        let (cond, params) = filter_sql(filters, true);
                        let allow: HashSet<i64> = {
                            let mut st = r
                                .prepare(&format!(
                                    "SELECT f.fid FROM files f WHERE 1=1{}",
                                    cond.replace("c.kind", "'x'")
                                ))
                                .map_err(db_err)?;
                            let it = st
                                .query_map(params_from_iter(params), |x| x.get(0))
                                .map_err(db_err)?;
                            it.collect::<std::result::Result<_, _>>().map_err(db_err)?
                        };
                        let nfiles: i64 = r
                            .query_row("SELECT COUNT(*) FROM files", [], |x| x.get(0))
                            .map_err(db_err)?;
                        let allow_opt = if allow.len() as i64 == nfiles {
                            None
                        } else {
                            Some(&allow)
                        };
                        let k = if filters.kinds.is_empty() {
                            per_list
                        } else {
                            per_list * 4
                        };
                        let hits = self.vecs.read().search(&q, k, allow_opt);
                        let kinds_ok: Option<HashSet<i64>> = if filters.kinds.is_empty() {
                            None
                        } else {
                            let ids: Vec<String> = hits.iter().map(|h| h.0.to_string()).collect();
                            let kinds: Vec<String> = filters
                                .kinds
                                .iter()
                                .map(|k| format!("'{}'", chunk_kind_str(*k)))
                                .collect();
                            let sql = format!(
                                "SELECT id FROM chunks WHERE id IN ({}) AND kind IN ({})",
                                ids.join(","),
                                kinds.join(",")
                            );
                            let mut st = r.prepare(&sql).map_err(db_err)?;
                            let it = st.query_map([], |x| x.get(0)).map_err(db_err)?;
                            Some(it.collect::<std::result::Result<_, _>>().map_err(db_err)?)
                        };
                        let mut rank = 0;
                        for (id, cos) in hits {
                            if cos < min_cos || kinds_ok.as_ref().is_some_and(|s| !s.contains(&id)) {
                                continue;
                            }
                            *fused.entry(id).or_default() += w_vec / (RRF_K + rank as f32 + 1.0);
                            rank += 1;
                            if rank >= per_list {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        // the vector half failed: say so (mode_used) and keep serving keywords
                        tracing::warn!(code = ?e.code, "query embedding failed; keyword only");
                        *self.last_error.lock() = Some(e);
                        drop(r);
                        return self.candidates(text, filters, SearchMode::Keyword, per_list, no_cloud_query);
                    }
                }
            }
        }
        if fused.is_empty() {
            return Ok((vec![], used));
        }
        let ids: Vec<String> = fused.keys().map(i64::to_string).collect();
        let sql = format!("{ROW_SQL} WHERE c.id IN ({})", ids.join(","));
        let mut st = r.prepare(&sql).map_err(db_err)?;
        let rows = st.query_map([], map_row).map_err(db_err)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let mut out = vec![];
        for row in rows {
            let row = row.map_err(db_err)?;
            let mut s = fused[&row.id];
            if row.kind == ChunkKind::Qa {
                s *= QA_WEIGHT;
            }
            if let Some(w) = row.when_ms {
                let age_days = ((now - w).max(0) as f32) / 86_400_000.0;
                s *= 1.0 + 0.1 * 0.5f32.powf(age_days / 180.0);
            }
            out.push(Cand { row, score: s });
        }
        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.row.id.cmp(&b.row.id))
        });
        Ok((out, used))
    }

    /// BM25 top `k` for the stemmed query. Very common stems (more than 20% of all chunks) carry
    /// no signal and make an OR query score the whole index, so they are dropped unless nothing
    /// else is left. All remaining stems are tried as AND first (cheap and precise when one is
    /// rare), then as OR to fill the list; AND hits rank first.
    fn fts_ranked(
        &self,
        r: &rusqlite::Connection,
        stems: &[String],
        cond: &str,
        params: &[Value],
        k: usize,
    ) -> Result<Vec<i64>> {
        let total: i64 = r
            .query_row("SELECT COALESCE(MAX(id),0) FROM chunks", [], |x| x.get(0))
            .map_err(db_err)?;
        let mut df: Vec<(i64, &String)> = vec![];
        {
            let mut st = r
                .prepare_cached("SELECT doc FROM chunks_vocab WHERE term=?1")
                .map_err(db_err)?;
            for s in stems {
                let d: i64 = st.query_row([s], |x| x.get(0)).unwrap_or(0);
                df.push((d, s));
            }
        }
        df.sort();
        // "Very common" only means something in a big index: below COMMON_STEM_FLOOR chunks every
        // stem is allowed, so a tiny vault still answers ordinary words.
        let limit = (total as f64 * 0.2) as i64;
        let mut keep: Vec<&String> = df
            .iter()
            .filter(|(d, _)| *d <= limit.max(COMMON_STEM_FLOOR))
            .map(|x| x.1)
            .collect();
        if keep.is_empty() {
            keep = df.iter().map(|x| x.1).collect();
        }
        let quote = |s: &String| format!("\"{}\"", s.replace('"', ""));
        let run = |m: String| -> Result<Vec<i64>> {
            let sql = format!(
                "SELECT chunks_fts.rowid FROM chunks_fts JOIN chunks c ON c.id=chunks_fts.rowid JOIN files f ON f.fid=c.fid
                 WHERE chunks_fts MATCH ?{cond} ORDER BY bm25(chunks_fts) LIMIT {k}"
            );
            let mut p = vec![Value::Text(m)];
            p.extend(params.iter().cloned());
            let mut st = r.prepare_cached(&sql).map_err(db_err)?;
            let it = st
                .query_map(params_from_iter(p), |x| x.get::<_, i64>(0))
                .map_err(db_err)?;
            it.collect::<std::result::Result<_, _>>().map_err(db_err)
        };
        let mut out: Vec<i64> = vec![];
        let present: Vec<&String> = keep
            .iter()
            .copied()
            .filter(|s| df.iter().any(|(d, t)| t == s && *d > 0))
            .collect();
        if present.len() >= 2 && present.len() == keep.len() {
            out = run(present.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" AND "))?;
        }
        if out.len() < k {
            let more = run(keep.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" OR "))?;
            let seen: HashSet<i64> = out.iter().copied().collect();
            out.extend(more.into_iter().filter(|i| !seen.contains(i)));
            out.truncate(k);
        }
        Ok(out)
    }

    pub(crate) fn search(&self, q: &SearchQuery) -> Result<SearchResults> {
        let t0 = Instant::now();
        let (cands, used) = self.candidates(&q.text, &q.filters, q.mode, TOP_EACH, false)?;
        let qstems: HashSet<String> = query_stems(&q.text).into_iter().collect();

        // merge neighbouring chunks of one file into one hit
        struct Group {
            members: Vec<usize>,
            score: f32,
        }
        let mut groups: Vec<Group> = vec![];
        for (i, c) in cands.iter().enumerate() {
            let hit = groups.iter_mut().find(|g| {
                g.members
                    .iter()
                    .any(|&m| cands[m].row.fid == c.row.fid && (cands[m].row.ord - c.row.ord).abs() == 1)
            });
            match hit {
                Some(g) => g.members.push(i),
                None => groups.push(Group {
                    members: vec![i],
                    score: c.score,
                }),
            }
        }
        let mut facet: HashMap<String, u32> = HashMap::new();
        for g in &groups {
            *facet.entry(cands[g.members[0]].row.folder.clone()).or_default() += 1;
        }
        let mut facets: Vec<FacetCount> = facet
            .into_iter()
            .map(|(k, count)| FacetCount {
                folder: parse_folder_key(&k),
                count,
            })
            .collect();
        facets.sort_by_key(|a| std::cmp::Reverse(a.count));

        let total = groups.len() as u32;
        let limit = if q.limit == 0 { 20 } else { q.limit.min(100) } as usize;
        let folder_titles = self.folder_titles()?;
        let hits: Vec<SearchHit> = groups
            .iter()
            .skip(q.offset as usize)
            .take(limit)
            .map(|g| {
                let lead = &cands[g.members[0]].row;
                let mut ms: Vec<&Row> = g.members.iter().map(|&m| &cands[m].row).collect();
                ms.sort_by_key(|r| r.ord);
                let joined = ms.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join("\n");
                let (snippet, highlights) = make_snippet(&joined, &qstems);
                let folder_title = folder_titles.get(&lead.folder).cloned().unwrap_or_else(|| {
                    match parse_folder_key(&lead.folder) {
                        FolderRef::Unsorted => "Unsorted".into(),
                        FolderRef::Trash => "Trash".into(),
                        FolderRef::User(_) => String::new(),
                    }
                });
                SearchHit {
                    chunk_id: lead.cid.clone(),
                    kind: lead.kind,
                    source: lead.source_ref(),
                    folder_title,
                    snippet,
                    highlights,
                    tags: lead.tags.clone(),
                    when: lead.when_ts.clone(),
                    score: g.score,
                }
            })
            .collect();
        Ok(SearchResults {
            hits,
            total,
            took_ms: t0.elapsed().as_millis() as u32,
            mode_used: used,
            facets,
        })
    }

    pub(crate) fn folder_titles(&self) -> Result<HashMap<String, String>> {
        let r = self.r.lock();
        let mut st = r
            .prepare_cached("SELECT key, title FROM folders")
            .map_err(db_err)?;
        let it = st
            .query_map([], |x| Ok((x.get::<_, String>(0)?, x.get::<_, String>(1)?)))
            .map_err(db_err)?;
        it.collect::<std::result::Result<_, _>>().map_err(db_err)
    }
}

/// ~280 characters around the first match, newlines flattened; highlight ranges are UTF-16
/// offsets into the returned snippet.
pub fn make_snippet(text: &str, qstems: &HashSet<String>) -> (String, Vec<(u32, u32)>) {
    const LEN: usize = 280;
    const LEAD: usize = 60;
    let flat = text.replace('\n', " ");
    let ws = words(&flat);
    let first = ws
        .iter()
        .find(|w| qstems.contains(&w.stem))
        .map(|w| w.start)
        .unwrap_or(0);
    let idx: Vec<usize> = flat.char_indices().map(|(i, _)| i).collect();
    let pos = idx.partition_point(|&b| b < first);
    let mut s = pos.saturating_sub(LEAD);
    // start on a word boundary
    while s > 0 && s < idx.len() && !flat[idx[s - 1]..idx[s]].chars().all(char::is_whitespace) {
        s -= 1;
        if pos - s > LEAD + 25 {
            break;
        }
    }
    let e = (s + LEN).min(idx.len());
    let b0 = idx.get(s).copied().unwrap_or(0);
    let b1 = if e >= idx.len() { flat.len() } else { idx[e] };
    let mut snip = String::new();
    if s > 0 {
        snip.push('…');
    }
    snip.push_str(flat[b0..b1].trim());
    if e < idx.len() {
        snip.push('…');
    }
    let hl = words(&snip)
        .into_iter()
        .filter(|w| qstems.contains(&w.stem))
        .map(|w| (utf16_offset(&snip, w.start), utf16_offset(&snip, w.end)))
        .collect();
    (snip, hl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_highlights_are_utf16_offsets() {
        let stems: HashSet<String> = query_stems("бюджет").into_iter().collect();
        let (s, h) = make_snippet("😀 мы обсудили бюджет проекта", &stems);
        assert_eq!(h.len(), 1);
        let u: Vec<u16> = s.encode_utf16().collect();
        let got = String::from_utf16(&u[h[0].0 as usize..h[0].1 as usize]).unwrap();
        assert_eq!(got, "бюджет");
    }

    #[test]
    fn long_text_is_cut_around_the_match() {
        let text = format!("{} needle {}", "filler ".repeat(100), "tail ".repeat(100));
        let stems: HashSet<String> = query_stems("needle").into_iter().collect();
        let (s, h) = make_snippet(&text, &stems);
        assert!(s.chars().count() < 320 && s.starts_with('…') && s.ends_with('…'));
        assert_eq!(h.len(), 1);
    }
}
