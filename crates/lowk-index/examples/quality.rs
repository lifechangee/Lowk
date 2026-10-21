//! Retrieval quality on the RU/EN/code-switch fixture (fixtures/index/eval.json).
//!   cargo run --release -p lowk-index --example quality            # keyword vs hash-fake (no model)
//!   LOWK_EVAL_MODEL=/path/embeddinggemma-2-Q8_0.gguf cargo run --release -p lowk-index --example quality
//! Env: LOWK_EVAL_MODEL (gguf), LOWK_EMBED_WORKER (llama-server build), LOWK_EVAL_DIM (default 512),
//!      LOWK_EVAL_MIN_COS (print what a cosine floor would drop).
//! Metric: MRR@5 and recall@5 of the relevant document (hit = any chunk of it among the top 5).
use lowk_index::embedder::local::find_worker;
use lowk_index::embedder::{HashEmbedder, LocalEmbedder};
use lowk_index::Index;
use lowk_types::*;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[derive(Deserialize)]
struct Doc {
    id: String,
    kind: String,
    title: String,
    md: String,
}
#[derive(Deserialize)]
struct Query {
    q: String,
    rel: String,
    #[serde(rename = "type")]
    typ: String,
}
#[derive(Deserialize)]
struct Eval {
    docs: Vec<Doc>,
    queries: Vec<Query>,
}

fn main() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/index/eval.json");
    let ev: Eval = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let dir = std::env::temp_dir().join(format!("lowk-index-eval-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ix = Index::open(&dir.join("index.db")).unwrap();
    let f = FolderRef::User(Ulid::from_parts(1_700_000_000_000, 1));
    ix.upsert_folder(&IndexFolder {
        folder: f.clone(),
        title: "Eval".into(),
        allow_cloud: true,
        instructions: String::new(),
        tags: vec![],
    })
    .unwrap();
    let mut ids = BTreeMap::new();
    for (i, d) in ev.docs.iter().enumerate() {
        let id = Ulid::from_parts(1_700_000_000_000 + 10 + i as u64, i as u128);
        ids.insert(id, d.id.clone());
        ix.upsert(&IndexItem {
            id,
            kind: if d.kind == "doc" {
                IndexItemKind::Doc
            } else {
                IndexItemKind::Call
            },
            folder: f.clone(),
            title: d.title.clone(),
            tags: vec![],
            when: None,
            md: d.md.clone(),
            content_hash: d.id.clone(),
            pinned: false,
            deleted: false,
        })
        .unwrap();
    }
    let dim: usize = std::env::var("LOWK_EVAL_DIM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(512);
    let (emb, name): (Arc<dyn lowk_index::Embedder>, String) = match std::env::var("LOWK_EVAL_MODEL") {
        Ok(m) => {
            let w = find_worker().expect("embedding worker not found (LOWK_EMBED_WORKER)");
            (
                Arc::new(LocalEmbedder::new(w, PathBuf::from(&m), dim)),
                format!("EmbeddingGemma 2 Q8_0 dim {dim}"),
            )
        }
        Err(_) => (
            Arc::new(HashEmbedder::new(dim)),
            "hash-fake (no semantics)".into(),
        ),
    };
    let t = Instant::now();
    ix.set_embedder(Some(emb), SemanticState::Off).unwrap();
    while ix.embed_pending(16).unwrap() > 0 {}
    let env = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (wf, wv, mc) = (
        env("LOWK_W_FTS", 1.0),
        env("LOWK_W_VEC", 1.0),
        env("LOWK_MIN_COS", 0.0),
    );
    ix.set_tuning(wf, wv, mc);
    println!("RRF weights fts {wf} vec {wv}, cosine floor {mc}");
    println!(
        "embedder: {name}; {} chunks embedded in {:.1}s",
        ix.status().chunks,
        t.elapsed().as_secs_f32()
    );

    let mut report: BTreeMap<(String, String), (f64, f64, u32)> = BTreeMap::new();
    let mut lat = vec![];
    for mode in [SearchMode::Keyword, SearchMode::Semantic, SearchMode::Hybrid] {
        for q in &ev.queries {
            let t = Instant::now();
            let r = ix
                .search(&SearchQuery {
                    text: q.q.clone(),
                    filters: SearchFilters::default(),
                    mode,
                    limit: 5,
                    offset: 0,
                })
                .unwrap();
            if mode == SearchMode::Hybrid {
                lat.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            let rank = r.hits.iter().position(|h| ids.get(&h.source.id) == Some(&q.rel));
            let (rr, hit) = rank.map(|p| (1.0 / (p as f64 + 1.0), 1.0)).unwrap_or((0.0, 0.0));
            for key in [q.typ.clone(), "all".to_string()] {
                let e = report.entry((format!("{mode:?}"), key)).or_default();
                e.0 += rr;
                e.1 += hit;
                e.2 += 1;
            }
        }
    }
    println!(
        "\n{:<10} {:<6} {:>8} {:>10} {:>5}",
        "mode", "type", "MRR@5", "recall@5", "n"
    );
    for ((mode, typ), (rr, hit, n)) in &report {
        println!(
            "{:<10} {:<6} {:>8.3} {:>10.3} {:>5}",
            mode,
            typ,
            rr / *n as f64,
            hit / *n as f64,
            n
        );
    }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "\nhybrid query latency (incl. query embedding): p50 {:.0} ms, p95 {:.0} ms",
        lat[lat.len() / 2],
        lat[lat.len() * 95 / 100]
    );
    let _ = std::fs::remove_dir_all(&dir);
}
