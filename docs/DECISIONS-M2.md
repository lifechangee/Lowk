# M2 decisions

## WP-C: index, hybrid search, embedder

**Index.** One SQLite file (`.lowk/index.db`, rebuildable): `files`, `chunks`, `chunks_fts` (FTS5 over text stemmed in Rust: Snowball RU+EN, script-boundary tokenisation, e/yo folded), `vecs`, `folders`. The index never reads the vault; the app feeds `IndexItem`s and diffs `known()` (reindex = blanked hashes, old rows keep serving). Chunk ids are `blake3(file id, text, n-th duplicate)`: byte-stable across reindex.

**Vectors.** `sqlite-vec` was not used: a flat in-memory int8 scan (one scale per vector, 512 dims via Matryoshka truncation, 51 MB at 100k chunks) behind `VectorIndex`; sqlite-vec can slot in later.

**Embedder runtime.** The `llama-cpp-2` crate (0.1.160) vendors a llama.cpp without the `gemma-embedding2` architecture, so the sidecar is upstream llama.cpp `llama-server --embedding` (tag v0.7.0, `workers/lowk-embed-worker/build.sh`, Metal on macOS, loopback HTTP). Model: ggml-org EmbeddingGemma 2 Q8_0, 309,855,456 bytes, SHA-256 pinned to a repo commit, resumable download (copied pattern of `lowk-stt::models`). Cloud: OpenAI-compatible `/embeddings`. Off = keyword only.

**Fusion.** RRF k=60, top-50 per list; FTS weight 0.7; FTS hits weighted by squared share of query words they contain (without this, one-word keyword noise outranked true semantic hits); very common stems (>20% of chunks) are dropped from the FTS query; AND first, OR to fill. Qa x0.6, recency boost up to +10%. `mode_used = keyword` while vectors are missing or building.

**Eval** (`fixtures/index/eval.json`, 102 queries over 46 passages, EmbeddingGemma 2 dim 512, Mac): MRR@5 / recall@5.

| mode | all | kw | paraphrase | cross-language |
|---|---|---|---|---|
| keyword | 0.503 / 0.549 | 1.00 | 0.39 | 0.12 |
| hybrid (shipped) | 0.928 / 0.990 | 1.00 | 0.92 | 0.86 |
| semantic only | 0.958 / 0.990 | 1.00 | 0.92 | 0.95 |

Hybrid beats keyword clearly; semantic-only edges hybrid on this tiny haystack (every passage is in the vector top-50), hybrid is kept for exact identifiers. Query embed latency 23 ms p50. Not run: bge-m3 and multilingual-e5-small comparison, i5-10400F timing (fallback plan unchanged).

**Latency** (`cargo run --release -p lowk-index --example bench`, 100,294 chunks, Zipfian RU/EN, fake embedder, M4 under heavy load from other agents): keyword p95 94 ms, hybrid p95 160 ms (p50 34 ms); add ~25 ms for the real query embedding.

**Cross-target CI.** rusqlite bundles C, so `just ci-cross` uses a no-op compiler and a fake archiver (clippy never links).

## WP-E: integration
* **Live model.** `llm.model` default = `deepseek/deepseek-v4.1-flash-1017` (was the unpinned slug: 2.3 s TTFT p50). One-time provider probe pins `llm.pinned_provider` (only when ask and summary model are the same slug). Fallback model: `anthropic/claude-haiku-5.5`. Measured p50 855 ms / p95 1165 ms (n=8).
* **Local-only policy** is a backend rule with four enforcement points (route, preflight, retrieval, cloud embedder); tests in `route.rs`, `lowk-index/tests/index.rs`, `lowk-session`, `selfcheck.rs`. A "local" URL must be loopback / private LAN / `.local`, otherwise it counts as cloud.
* **Bundle diagnostics.** `lowk --selfcheck-m2` is the only way to drive the release binary headlessly (no QA channel exists in releases). It uses the same `Services` as the app.
* **Embedder sidecar** ships as externalBin `lowk-embed-worker` (20.7 MB), model on demand.
