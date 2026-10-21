//! Real EmbeddingGemma 2 through the llama.cpp sidecar. Needs the model and the worker:
//!   LOWK_EVAL_MODEL=/path/embeddinggemma-2-Q8_0.gguf LOWK_EMBED_WORKER=/path/lowk-embed-worker \
//!     cargo test -p lowk-index --test real_embedder -- --ignored
use lowk_index::embedder::local::find_worker;
use lowk_index::embedder::LocalEmbedder;
use lowk_index::Embedder;
use std::path::PathBuf;

fn cos(a: &[f32], b: &[f32]) -> f32 {
    let d: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    d / (a.iter().map(|x| x * x).sum::<f32>().sqrt() * b.iter().map(|x| x * x).sum::<f32>().sqrt())
}

#[test]
#[ignore = "needs the model and the worker"]
fn embeds_and_ranks_across_languages() {
    let model = PathBuf::from(std::env::var("LOWK_EVAL_MODEL").expect("LOWK_EVAL_MODEL"));
    let e = LocalEmbedder::new(find_worker().expect("worker"), model, 512);
    let q = e
        .embed_query(&e.format_query("штраф за просрочку поставки"))
        .unwrap();
    assert_eq!(q.len(), 512);
    let docs = e
        .embed_docs(&[
            e.format_doc(
                "Договор",
                "The supplier pays a late delivery penalty of 0.1 percent per day.",
            ),
            e.format_doc(
                "Обед",
                "Обсуждали, куда пойти на обед: итальянский ресторан или столовая.",
            ),
        ])
        .unwrap();
    assert!(
        cos(&q, &docs[0]) > cos(&q, &docs[1]),
        "cross-language relevance should win"
    );
}
