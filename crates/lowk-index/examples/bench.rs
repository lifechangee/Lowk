//! Search latency on a synthetic year-scale vault.
//!   cargo run --release -p lowk-index --example bench -- [chunks=100000] [queries=300] [dim=512]
//! Zipfian RU/EN vocabulary, calls and documents spread over 40 folders, a deterministic fake
//! embedder (the vector scan cost is what is measured; the real embedder adds one query
//! encode, reported separately by the `embed_eval` example).
use lowk_index::embedder::HashEmbedder;
use lowk_index::Index;
use lowk_types::*;
use std::sync::Arc;
use std::time::Instant;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn vocab(rng: &mut Rng, n: usize) -> Vec<String> {
    let ru = [
        "ба", "ко", "ре", "ми", "ту", "ло", "ша", "ни", "до", "вер", "сти", "про", "гра", "мен", "тор", "бю",
        "джет",
    ];
    let en = [
        "ba", "co", "re", "mi", "tu", "lo", "sha", "ni", "do", "ver", "sti", "pro", "gra", "men", "tor",
        "bu", "get",
    ];
    (0..n)
        .map(|i| {
            let syl = if i % 3 == 0 { &en } else { &ru };
            let k = 2 + rng.below(3);
            let mut w: String = (0..k).map(|_| syl[rng.below(syl.len())]).collect();
            w.push_str(["ть", "ing", "ы", "s", "ов", ""][rng.below(6)]);
            w
        })
        .collect()
}

fn main() {
    let args: Vec<usize> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let target = args.first().copied().unwrap_or(100_000);
    let nq = args.get(1).copied().unwrap_or(300);
    let dim = args.get(2).copied().unwrap_or(512);
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let words = vocab(&mut rng, 30_000);
    // Zipf: P(rank r) ~ 1/r
    let cum: Vec<f64> = {
        let mut s = 0.0;
        words
            .iter()
            .enumerate()
            .map(|(i, _)| {
                s += 1.0 / (i as f64 + 1.0);
                s
            })
            .collect()
    };
    let total = *cum.last().unwrap();
    let pick = |rng: &mut Rng| -> &str {
        let x = rng.unit() * total;
        &words[cum.partition_point(|c| *c < x).min(words.len() - 1)]
    };

    let dir = std::env::temp_dir().join(format!("lowk-index-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ix = Index::open(&dir.join("index.db")).unwrap();
    ix.set_embedder(Some(Arc::new(HashEmbedder::new(dim))), SemanticState::Off)
        .unwrap();
    let folders: Vec<FolderRef> = (0..40)
        .map(|i| FolderRef::User(Ulid::from_parts(1_700_000_000_000 + i, i as u128)))
        .collect();
    for (i, f) in folders.iter().enumerate() {
        ix.upsert_folder(&IndexFolder {
            folder: f.clone(),
            title: format!("Folder {i}"),
            allow_cloud: true,
            instructions: String::new(),
            tags: vec![],
        })
        .unwrap();
    }

    let t = Instant::now();
    let mut chunks = 0usize;
    let mut n = 0u64;
    while chunks < target {
        n += 1;
        let is_call = n % 3 != 0;
        let mut md = String::from("## Transcript\n\n");
        let turns = 40 + rng.below(40);
        for k in 0..turns {
            let len = 8 + rng.below(25);
            let s: Vec<&str> = (0..len).map(|_| pick(&mut rng)).collect();
            if is_call {
                md.push_str(&format!(
                    "[{:02}:{:02}:{:02}] **System:** {}\n",
                    k / 360,
                    (k / 6) % 60,
                    (k % 6) * 10,
                    s.join(" ")
                ));
            } else {
                md.push_str(&format!("{}\n\n", s.join(" ")));
                if k % 12 == 0 {
                    md.push_str(&format!("<!-- p:{} -->\n", k / 12 + 1));
                }
            }
        }
        let item = IndexItem {
            id: Ulid::from_parts(1_700_000_000_000 + 1000 + n, n as u128),
            kind: if is_call {
                IndexItemKind::Call
            } else {
                IndexItemKind::Doc
            },
            folder: folders[rng.below(40)].clone(),
            title: format!("Item {n}"),
            tags: vec![],
            when: Some(format!(
                "2026-{:02}-{:02}T10:00:00+00:00",
                1 + rng.below(10),
                1 + rng.below(27)
            )),
            content_hash: format!("h{n}"),
            md,
            pinned: false,
            deleted: false,
        };
        ix.upsert(&item).unwrap();
        if n % 100 == 0 {
            chunks = ix.status().chunks as usize;
        }
    }
    println!(
        "ingest: {} chunks from {} items in {:.1}s (chunk + FTS)",
        chunks,
        n,
        t.elapsed().as_secs_f32()
    );
    let t = Instant::now();
    while ix.embed_pending(256).unwrap() > 0 {}
    println!("embed (fake, dim {dim}): {:.1}s", t.elapsed().as_secs_f32());
    assert_eq!(ix.status().semantic, SemanticState::Ready);

    for mode in [SearchMode::Keyword, SearchMode::Hybrid] {
        for filtered in [false, true] {
            let mut lat: Vec<f64> = vec![];
            for _ in 0..nq {
                let k = 2 + rng.below(3);
                let text: Vec<&str> = (0..k).map(|_| pick(&mut rng)).collect();
                let mut f = SearchFilters::default();
                if filtered {
                    f.folders = vec![folders[rng.below(40)].clone()];
                }
                let t = Instant::now();
                let r = ix
                    .search(&SearchQuery {
                        text: text.join(" "),
                        filters: f,
                        mode,
                        limit: 20,
                        offset: 0,
                    })
                    .unwrap();
                lat.push(t.elapsed().as_secs_f64() * 1000.0);
                assert!(r.hits.len() <= 20);
            }
            lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let p = |q: f64| lat[((lat.len() as f64 * q) as usize).min(lat.len() - 1)];
            println!(
                "search {:?}{}: p50 {:.1} ms  p95 {:.1} ms  max {:.1} ms  ({} queries, {} chunks)",
                mode,
                if filtered { " +folder filter" } else { "" },
                p(0.5),
                p(0.95),
                lat[lat.len() - 1],
                nq,
                chunks
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
