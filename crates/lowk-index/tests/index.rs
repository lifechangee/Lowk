#![allow(clippy::field_reassign_with_default)]
use lowk_index::embedder::HashEmbedder;
use lowk_index::Index;
use lowk_types::*;
use std::sync::Arc;

pub fn uid(n: u64) -> Ulid {
    Ulid::from_parts(1_700_000_000_000 + n, n as u128)
}

fn call_md(lines: &[(&str, &str)]) -> String {
    let mut s = String::from("# Call\n\n## Transcript\n\n");
    for (i, (who, t)) in lines.iter().enumerate() {
        s.push_str(&format!("[00:{:02}:{:02}] **{who}:** {t}\n", i / 6, (i % 6) * 10));
    }
    s
}

fn item(n: u64, kind: IndexItemKind, folder: FolderRef, title: &str, md: String) -> IndexItem {
    IndexItem {
        id: uid(n),
        kind,
        folder,
        title: title.into(),
        tags: vec![],
        when: Some("2026-10-01T10:00:00+00:00".into()),
        content_hash: blake3::hash(md.as_bytes()).to_hex().to_string(),
        md,
        pinned: false,
        deleted: false,
    }
}

fn folder(f: &FolderRef, title: &str, cloud: bool, ins: &str) -> IndexFolder {
    IndexFolder {
        folder: f.clone(),
        title: title.into(),
        allow_cloud: cloud,
        instructions: ins.into(),
        tags: vec![],
    }
}

fn q(text: &str) -> SearchQuery {
    SearchQuery {
        text: text.into(),
        filters: SearchFilters::default(),
        mode: SearchMode::Hybrid,
        limit: 10,
        offset: 0,
    }
}

fn fixture() -> (tempfile::TempDir, Index, FolderRef, FolderRef) {
    let dir = tempfile::tempdir().unwrap();
    let ix = Index::open(&dir.path().join("index.db")).unwrap();
    let fa = FolderRef::User(uid(100));
    let fb = FolderRef::User(uid(101));
    ix.upsert_folder(&folder(
        &fa,
        "Alpha",
        true,
        "Client Alpha is a fintech. Answer formally.",
    ))
    .unwrap();
    ix.upsert_folder(&folder(&fb, "Beta", false, "Private folder."))
        .unwrap();
    ix.upsert(&item(
        1,
        IndexItemKind::Call,
        fa.clone(),
        "Weekly sync",
        call_md(&[
            ("Me", "Привет, слышно меня?"),
            (
                "System",
                "Давайте обсудим бюджет проекта и сроки поставки оборудования.",
            ),
            ("System", "А что значит EBITDA в вашем отчёте за квартал?"),
        ]),
    ))
    .unwrap();
    ix.upsert(&item(
        2,
        IndexItemKind::Doc,
        fa.clone(),
        "Spec.pdf",
        "<!-- p:1 -->\n# Overview\n\nThe integration exposes webhooks for invoices.\n\n<!-- p:2 -->\n## Limits\n\nRate limit is 100 requests per minute per API key.\n".into(),
    ))
    .unwrap();
    ix.upsert(&item(
        3,
        IndexItemKind::Call,
        fb.clone(),
        "Secret call",
        call_md(&[("System", "Конфиденциально: бюджет на слияние компаний утверждён.")]),
    ))
    .unwrap();
    (dir, ix, fa, fb)
}

#[test]
fn keyword_search_stems_ru_and_en_and_cites_sources() {
    let (_d, ix, fa, _) = fixture();
    let r = ix.search(&q("бюджетом проектов")).unwrap();
    assert_eq!(r.mode_used, SearchMode::Keyword);
    let top = &r.hits[0];
    assert_eq!(top.source.kind, RefKind::Call);
    assert_eq!(top.source.id, uid(1));
    assert_eq!(top.source.folder, fa);
    assert_eq!(top.source.at_s, Some(0));
    assert!(!top.highlights.is_empty());
    let r = ix.search(&q("rate limits")).unwrap();
    assert_eq!(r.hits[0].source.kind, RefKind::Doc);
    assert_eq!(r.hits[0].source.page, Some(2));
    assert_eq!(
        r.hits[0].source.heading_path,
        vec!["Overview".to_string(), "Limits".to_string()]
    );
    // code switching: Latin term inside a Russian query
    let r = ix.search(&q("что значит EBITDA")).unwrap();
    assert_eq!(r.hits[0].source.id, uid(1));
    assert!(r.hits[0].source.at_s.is_some());
}

#[test]
fn filters_folder_tag_date_kind_trash() {
    let (_d, ix, fa, fb) = fixture();
    let mut f = SearchFilters::default();
    f.folders = vec![fb.clone()];
    let r = ix
        .search(&SearchQuery {
            filters: f,
            ..q("бюджет")
        })
        .unwrap();
    assert!(r.hits.iter().all(|h| h.source.folder == fb) && !r.hits.is_empty());
    let all = ix.search(&q("бюджет")).unwrap();
    assert!(all.facets.len() == 2);

    let mut it = item(
        4,
        IndexItemKind::Doc,
        fa.clone(),
        "Tagged",
        "# T\n\nbudget notes about hiring".into(),
    );
    it.tags = vec!["HR".into()];
    it.when = Some("2026-03-05T00:00:00+00:00".into());
    ix.upsert(&it).unwrap();
    let mut f = SearchFilters::default();
    f.tags = vec!["hr".into()];
    assert_eq!(
        ix.search(&SearchQuery {
            filters: f,
            ..q("budget")
        })
        .unwrap()
        .hits
        .len(),
        1
    );
    let mut f = SearchFilters::default();
    f.from = Some("2026-03-01".into());
    f.to = Some("2026-03-05".into());
    let r = ix
        .search(&SearchQuery {
            filters: f,
            ..q("budget")
        })
        .unwrap();
    assert_eq!(r.hits.len(), 1);
    let mut f = SearchFilters::default();
    f.kinds = vec![ChunkKind::Speech];
    let r = ix
        .search(&SearchQuery {
            filters: f,
            ..q("budget бюджет")
        })
        .unwrap();
    assert!(r.hits.iter().all(|h| h.kind == ChunkKind::Speech));

    // trash: hidden unless asked
    ix.remove(&uid(4), false).unwrap();
    assert!(ix.search(&q("hiring")).unwrap().hits.is_empty());
    let mut f = SearchFilters::default();
    f.include_trash = true;
    assert_eq!(
        ix.search(&SearchQuery {
            filters: f,
            ..q("hiring")
        })
        .unwrap()
        .hits
        .len(),
        1
    );
    ix.remove(&uid(4), true).unwrap();
    let mut f = SearchFilters::default();
    f.include_trash = true;
    assert!(ix
        .search(&SearchQuery {
            filters: f,
            ..q("hiring")
        })
        .unwrap()
        .hits
        .is_empty());
}

#[test]
fn incremental_upsert_skips_unchanged_and_keeps_chunk_ids() {
    let (_d, ix, fa, _) = fixture();
    let before = ix.search(&q("webhooks invoices")).unwrap().hits[0]
        .chunk_id
        .clone();
    let st = ix.status();
    // same hash: nothing changes
    let same = item(
        2,
        IndexItemKind::Doc,
        fa.clone(),
        "Spec.pdf",
        "<!-- p:1 -->\n# Overview\n\nThe integration exposes webhooks for invoices.\n\n<!-- p:2 -->\n## Limits\n\nRate limit is 100 requests per minute per API key.\n".into(),
    );
    ix.upsert(&same).unwrap();
    assert_eq!(ix.status().chunks, st.chunks);
    // edit the second page only: the first chunk id survives
    let mut edited = same.clone();
    edited.md = edited.md.replace("100 requests", "500 requests");
    edited.content_hash = "changed".into();
    ix.upsert(&edited).unwrap();
    assert_eq!(
        ix.search(&q("webhooks invoices")).unwrap().hits[0].chunk_id,
        before
    );
    assert_eq!(ix.search(&q("500 requests")).unwrap().hits[0].source.id, uid(2));
    assert!(ix
        .search(&q("100 requests"))
        .unwrap()
        .hits
        .iter()
        .all(|h| !h.snippet.contains("100 requests")));
}

#[test]
fn index_is_rebuildable_from_nothing() {
    let (d, ix, ..) = fixture();
    let a = ix.search(&q("budget бюджет EBITDA webhooks")).unwrap();
    let known = ix.known().unwrap();
    assert_eq!(known.len(), 3);
    drop(ix);
    // delete the file: a fresh index starts empty, `known` is empty so the app re-feeds
    for f in std::fs::read_dir(d.path()).unwrap() {
        std::fs::remove_file(f.unwrap().path()).unwrap();
    }
    let (_d2, ix2, ..) = fixture();
    let b = ix2.search(&q("budget бюджет EBITDA webhooks")).unwrap();
    let ids = |r: &SearchResults| {
        r.hits
            .iter()
            .map(|h| (h.source.id, h.source.at_s, h.source.page))
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&a), ids(&b));
    assert_eq!(
        a.hits.iter().map(|h| h.chunk_id.clone()).collect::<Vec<_>>(),
        b.hits.iter().map(|h| h.chunk_id.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn reindex_all_blanks_hashes_but_keeps_serving() {
    let (_d, ix, ..) = fixture();
    ix.reindex(ReindexScope::All).unwrap();
    assert_eq!(ix.status().pending, 3);
    assert!(ix.known().unwrap().iter().all(|(_, h)| h.is_empty()));
    assert!(!ix.search(&q("webhooks")).unwrap().hits.is_empty());
}

#[test]
fn hybrid_uses_vectors_when_ready_and_says_keyword_while_building() {
    let (_d, ix, ..) = fixture();
    let emb = Arc::new(
        HashEmbedder::new(256).with_synonyms(&[&["invoice", "bill", "счёт"], &["limit", "quota", "лимит"]]),
    );
    ix.set_embedder(Some(emb), SemanticState::Off).unwrap();
    assert!(matches!(ix.status().semantic, SemanticState::Building { .. }));
    let r = ix.search(&q("bill quota")).unwrap();
    assert_eq!(
        r.mode_used,
        SearchMode::Keyword,
        "no silent downgrade: partial vectors = keyword"
    );
    while ix.embed_pending(2).unwrap() > 0 {}
    assert_eq!(ix.status().semantic, SemanticState::Ready);
    // no lexical overlap with the text, only the synonym groups
    let r = ix.search(&q("bill quota")).unwrap();
    assert_eq!(r.mode_used, SearchMode::Hybrid);
    assert_eq!(r.hits[0].source.kind, RefKind::Doc);
    // keyword mode stays available on demand
    let r = ix
        .search(&SearchQuery {
            mode: SearchMode::Keyword,
            ..q("bill quota")
        })
        .unwrap();
    assert!(r.hits.is_empty());
    // model change: vectors are dropped, FTS keeps working
    ix.set_embedder(Some(Arc::new(HashEmbedder::new(128))), SemanticState::Off)
        .unwrap();
    assert!(matches!(
        ix.status().semantic,
        SemanticState::Building { done: 0, .. }
    ));
    assert!(!ix.search(&q("webhooks")).unwrap().hits.is_empty());
    // off
    ix.set_embedder(None, SemanticState::Off).unwrap();
    assert_eq!(ix.status().semantic, SemanticState::Off);
    assert_eq!(ix.search(&q("webhooks")).unwrap().mode_used, SearchMode::Keyword);
}

fn rq(folder: Option<Ulid>, text: &str) -> RetrievalQuery {
    RetrievalQuery {
        text: text.into(),
        scope: RetrievalScope {
            folder,
            call: None,
            tags: vec![],
            all_folders: false,
        },
        budget_tokens: 2000,
        exclude: vec![],
        cloud_provider: false,
        pinned_budget_tokens: 6000,
        include_folder_context: true,
        include_previous_calls: true,
    }
}

#[test]
fn retrieve_pack_instructions_pinned_docs_previous_calls() {
    let (_d, ix, fa, _) = fixture();
    let mut glossary = item(
        5,
        IndexItemKind::Doc,
        fa.clone(),
        "Glossary.md",
        "# Terms\n\nARR means annual recurring revenue.".into(),
    );
    glossary.pinned = true;
    ix.upsert(&glossary).unwrap();
    let FolderRef::User(fid) = fa.clone() else {
        unreachable!()
    };
    // the live call is itself indexed (weekly sync): it must not come back
    let mut q1 = rq(Some(fid), "бюджет проекта сроки поставки");
    q1.scope.call = Some(uid(1));
    let p = ix.retrieve_pack(&q1).unwrap();
    assert!(p.instructions.as_ref().unwrap().text.contains("fintech"));
    assert_eq!(
        p.instructions.as_ref().unwrap().source_ref.kind,
        RefKind::Instructions
    );
    assert_eq!(p.pinned.len(), 1);
    assert!(p.pinned[0].pinned && p.pinned[0].source_ref.pinned);
    assert!(
        p.relevant.iter().all(|c| c.source_ref.id != uid(1)),
        "current call excluded"
    );
    assert!(
        p.relevant.iter().all(|c| c.source_ref.folder == fa),
        "other folders not mixed in"
    );
    // a previous call of the same folder comes through
    ix.upsert(&item(
        6,
        IndexItemKind::Call,
        fa.clone(),
        "Earlier call",
        call_md(&[(
            "System",
            "Мы согласовали бюджет проекта в пределах трёх миллионов.",
        )]),
    ))
    .unwrap();
    let p = ix.retrieve_pack(&q1).unwrap();
    let prev = p
        .relevant
        .iter()
        .find(|c| c.source_ref.id == uid(6))
        .expect("previous call");
    assert_eq!(prev.source_ref.kind, RefKind::Call);
    assert_eq!(prev.source_ref.at_s, Some(0));
    assert!(prev.tokens > 0);
    // previous calls can be switched off
    let mut q2 = q1.clone();
    q2.include_previous_calls = false;
    assert!(ix
        .retrieve_pack(&q2)
        .unwrap()
        .relevant
        .iter()
        .all(|c| c.source_ref.kind == RefKind::Doc));
    // chunks already sent are not sent again
    let mut q3 = q1.clone();
    q3.exclude = vec![prev.id.clone()];
    assert!(ix
        .retrieve_pack(&q3)
        .unwrap()
        .relevant
        .iter()
        .all(|c| c.id != prev.id));
    // plain retrieve = relevant only
    assert!(ix.retrieve(&q1).unwrap().iter().any(|c| c.id == prev.id));
}

#[test]
fn budgets_pack_within_limits_and_report_dropped_pinned() {
    let (_d, ix, fa, _) = fixture();
    let FolderRef::User(fid) = fa.clone() else {
        unreachable!()
    };
    let long = format!("# Big\n\n{}", "alpha beta gamma delta ".repeat(400));
    for n in 10..13 {
        let mut it = item(
            n,
            IndexItemKind::Doc,
            fa.clone(),
            &format!("Pinned{n}"),
            long.clone(),
        );
        it.pinned = true;
        ix.upsert(&it).unwrap();
    }
    let mut query = rq(Some(fid), "alpha");
    query.pinned_budget_tokens = 3000;
    let p = ix.retrieve_pack(&query).unwrap();
    let used: u32 = p.pinned.iter().map(|c| c.tokens).sum();
    assert!(used <= 3000, "{used}");
    assert!(p.pinned_dropped >= 1);
    query.budget_tokens = 150;
    let p = ix.retrieve_pack(&query).unwrap();
    assert!(p.relevant.iter().map(|c| c.tokens).sum::<u32>() <= 150);
}

#[test]
fn cloud_policy_withholds_private_folders() {
    let (_d, ix, fa, fb) = fixture();
    let FolderRef::User(b) = fb.clone() else {
        unreachable!()
    };
    let mut q = rq(Some(b), "бюджет слияние компаний");
    q.cloud_provider = true;
    let p = ix.retrieve_pack(&q).unwrap();
    assert!(p.relevant.is_empty() && p.pinned.is_empty() && p.instructions.is_none());
    assert!(p.withheld >= 2, "{}", p.withheld);
    // the same query for a local model sees the folder
    q.cloud_provider = false;
    assert!(!ix.retrieve_pack(&q).unwrap().relevant.is_empty());
    // all_folders from an allowed folder still withholds the private one
    let FolderRef::User(a) = fa else { unreachable!() };
    let mut q = rq(Some(a), "бюджет слияние компаний");
    q.cloud_provider = true;
    q.scope.all_folders = true;
    let p = ix.retrieve_pack(&q).unwrap();
    assert!(p.relevant.iter().all(|c| c.source_ref.id != uid(3)));
    assert!(p.withheld >= 1);
    // an unknown folder is treated as private
    let ghost = rq(Some(uid(999)), "бюджет");
    let mut g = ghost.clone();
    g.cloud_provider = true;
    g.scope.all_folders = true;
    let p = ix.retrieve_pack(&g).unwrap();
    assert!(
        p.relevant.is_empty()
            || p.relevant
                .iter()
                .all(|c| c.source_ref.folder != FolderRef::User(uid(999)))
    );
}

#[test]
fn warm_up_serves_the_cached_pack_until_the_index_changes() {
    let (_d, ix, fa, _) = fixture();
    let FolderRef::User(f) = fa.clone() else {
        unreachable!()
    };
    let q = rq(Some(f), "webhooks");
    ix.warm_up(&q).unwrap();
    let a = ix.retrieve_pack(&q).unwrap();
    ix.upsert(&item(
        20,
        IndexItemKind::Doc,
        fa,
        "New",
        "# N\n\nwebhooks retry policy".into(),
    ))
    .unwrap();
    let b = ix.retrieve_pack(&q).unwrap();
    assert!(b.relevant.len() > a.relevant.len());
}

#[test]
fn qa_chunks_are_down_weighted_and_never_retrieved() {
    let (_d, ix, fa, _) = fixture();
    let FolderRef::User(f) = fa.clone() else {
        unreachable!()
    };
    let md = "## Transcript\n\n[00:00:03] **System:** zebra migration plan\n\n> [!lowk-qa] at=00:00:10\n> **Вопрос:** zebra migration plan\n> **Подсказка ИИ:** zebra migration plan details\n";
    ix.upsert(&item(30, IndexItemKind::Call, fa, "Zebra", md.into()))
        .unwrap();
    let r = ix.search(&q("zebra migration")).unwrap();
    assert_eq!(r.hits[0].kind, ChunkKind::Speech);
    let mut flt = SearchFilters::default();
    flt.kinds = vec![ChunkKind::Qa];
    let qa = ix
        .search(&SearchQuery {
            filters: flt,
            ..q("zebra migration")
        })
        .unwrap();
    assert!(
        qa.hits.iter().all(|h| h.kind == ChunkKind::Qa) && !qa.hits.is_empty(),
        "still findable"
    );
    let p = ix.retrieve_pack(&rq(Some(f), "zebra migration")).unwrap();
    assert!(p.relevant.iter().all(|c| c.kind != ChunkKind::Qa));
}

#[test]
fn concurrent_search_while_indexing() {
    let (_d, ix, fa, _) = fixture();
    let w = {
        let ix = ix.clone();
        std::thread::spawn(move || {
            for n in 50..80 {
                ix.upsert(&item(
                    n,
                    IndexItemKind::Doc,
                    fa.clone(),
                    "Bulk",
                    format!("# B\n\nbulk document number {n} about forecasts"),
                ))
                .unwrap();
            }
        })
    };
    for _ in 0..50 {
        ix.search(&q("forecasts")).unwrap();
    }
    w.join().unwrap();
    assert!(ix.search(&q("forecasts")).unwrap().total >= 30);
}

#[test]
fn background_thread_embeds_and_emits_status() {
    let (_d, ix, ..) = fixture();
    let seen = Arc::new(std::sync::atomic::AtomicU32::new(0));
    {
        let seen = seen.clone();
        ix.on_status(Box::new(move |s| {
            if s.semantic == SemanticState::Ready {
                seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }));
    }
    ix.start_background();
    ix.set_embedder(Some(Arc::new(HashEmbedder::new(64))), SemanticState::Off)
        .unwrap();
    let t = std::time::Instant::now();
    while ix.status().semantic != SemanticState::Ready {
        assert!(t.elapsed().as_secs() < 20, "embedding did not finish");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // paused: new content stays pending until resumed
    ix.pause_embedding(true);
    ix.upsert(&item(
        40,
        IndexItemKind::Doc,
        FolderRef::Unsorted,
        "Late",
        "# L\n\nlate arrival".into(),
    ))
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(matches!(ix.status().semantic, SemanticState::Building { .. }));
    ix.pause_embedding(false);
    let t = std::time::Instant::now();
    while ix.status().semantic != SemanticState::Ready {
        assert!(t.elapsed().as_secs() < 20);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        seen.load(std::sync::atomic::Ordering::Relaxed) >= 1,
        "a Ready status event was emitted"
    );
}

/// Records every text a "cloud" embedder is asked to encode.
struct SpyCloud {
    inner: HashEmbedder,
    docs: std::sync::Mutex<Vec<String>>,
    queries: std::sync::Mutex<Vec<String>>,
}
impl lowk_index::Embedder for SpyCloud {
    fn id(&self) -> &str {
        "spy-cloud"
    }
    fn dim(&self) -> usize {
        self.inner.dim()
    }
    fn is_cloud(&self) -> bool {
        true
    }
    fn format_doc(&self, _t: &str, text: &str) -> String {
        text.to_string()
    }
    fn format_query(&self, q: &str) -> String {
        q.to_string()
    }
    fn embed_docs(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // queries and documents both come through here (embed_query default); tell them apart by the caller
        let mut g = self.docs.lock().unwrap();
        g.extend(texts.iter().cloned());
        self.inner.embed_docs(texts)
    }
    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        self.queries.lock().unwrap().push(text.to_string());
        self.inner
            .embed_docs(&[text.to_string()])
            .map(|mut v| v.remove(0))
    }
}

/// Local-only policy (BRIEF): a folder with `allow_cloud = false` is never sent to a cloud
/// embedder, neither its chunks nor a retrieval query made from its transcript.
#[test]
fn local_only_folders_never_reach_a_cloud_embedder() {
    let (_d, ix, fa, fb) = fixture();
    let spy = Arc::new(SpyCloud {
        inner: HashEmbedder::new(64),
        docs: Default::default(),
        queries: Default::default(),
    });
    ix.set_embedder(Some(spy.clone()), SemanticState::Off).unwrap();
    while ix.embed_pending(4).unwrap() > 0 {}
    let docs = spy.docs.lock().unwrap().join("\n");
    assert!(docs.contains("webhooks"), "allowed folders are embedded");
    assert!(
        !docs.contains("слияние") && !docs.contains("Конфиденциально"),
        "local-only chunks were sent to the cloud embedder"
    );
    // semantic is Ready although the private chunks have no vectors (they are out of scope)
    assert_eq!(ix.status().semantic, SemanticState::Ready);
    // retrieval for the private folder: no cloud query
    let FolderRef::User(b) = fb else { unreachable!() };
    let before = spy.queries.lock().unwrap().len();
    let p = ix.retrieve_pack(&rq(Some(b), "бюджет слияние компаний")).unwrap();
    assert!(!p.relevant.is_empty(), "keyword retrieval still works locally");
    assert_eq!(
        spy.queries.lock().unwrap().len(),
        before,
        "query leaked to the cloud"
    );
    // an allowed folder does use it
    let FolderRef::User(a) = fa else { unreachable!() };
    let _ = ix.retrieve_pack(&rq(Some(a), "rate limit per minute")).unwrap();
    assert!(spy.queries.lock().unwrap().len() > before);
}
