# M2 status (WP-E integration done)

| # | Exit criterion (M2-PLAN section 2) | Status | Evidence |
|---|---|---|---|
| 1 | Search p95 < 200 ms at 100k chunks; index rebuildable | done | WP-C bench (hybrid p95 160 ms); selfcheck "index rebuilds from the vault" (deleted `index.db`, identical chunk ids) |
| 2 | Ingest corpus, every file converts or fails with a shown error | done (WP-B corpus); wired | queue with progress/cancel/retry/crash recovery; qa-bundle ingests PDF, XLSX, DOCX, PPTX, CSV, MD and a scanned PDF (Vision OCR) from the bundle |
| 3 | Hybrid beats keyword | done (WP-C, DECISIONS-M2) | bundle: hybrid finds the PDF by meaning with the bundled sidecar |
| 4 | `retrieve_pack` within budgets, cloud policy | done + used by Ask | session tests (pack, sources, exclude on 2nd turn); local-only tests in lowk-index, route.rs, selfcheck |
| 5 | Summary + tags via OpenRouter | done | golden tests (WP-D); selfcheck against a stub; live ask against OpenRouter in qa-bundle |
| 6 | UI variant A, RU+EN, visual suite | done | `just test-ui-visual` 1073 passed (baselines re-recorded); overlay source chips / no-key / local-only messages |
| 7 | Release gate: ci + ci-cross + bundle QA | local green; GitHub run pending the push | `just ci` (incl. ci-cross) green; `just qa-bundle --silent`: 65 pass, 0 fail, 1 NEEDS-GRANT (System Audio permission, owner action) |
| 8 | Owner-bug regression checks | none reported in M2 | |

## WP-E delivered
* Stubs replaced by `crates/lowk-app/src/m2/*` (no Tauri inside, so it runs in tests and in the bundle's headless `--selfcheck-m2`): ingest queue (2 workers), index feed (start-up reconcile by content hash + incremental upserts on every store change, `Indexing -> Ready`), embedder (install with SHA pin and progress, warm-up, pause while recording, off/local/cloud), summary + tags, key test, provider probe, pinned documents and folder instructions in the index.
* Ask: the session retrieves (`retrieve_pack`: instructions, pinned, relevant documents, previous calls of the folder, current call excluded), `LlmHints` builds the prompt (append-only ledger per call, custom templates, language of the question, «Мой ответ»), emits `llm://sources` before the first delta, streams, supports cancel. Overlay: source chips (click opens the call/document in the main window), clear messages for no API key (link to Settings) and local-only folders. No stub path remains.
* Local-only: enforced in the backend at four points: `route::resolve` (cloud route refused with `LocalOnlyBlocked` unless a local LLM is configured: `llm.local_base_url` + `llm.local_model`, or a local provider), preflight before retrieval, retrieval withholds the folder from cloud providers, the cloud embedder never embeds its chunks nor receives its queries. Summary and tags follow the same route.
* Native dialogs via tauri-plugin-dialog; export/import accept zip.
* Embedder sidecar bundled (`tools/build-embed-worker.sh`, externalBin, signed): bundle 50 MB (app 20.9, stt worker 8.1, embed worker 20.7); model (310 MB, SHA-256 pinned) downloads on demand.

## LLM latency (item 7)
Root cause: `llm.model` defaulted to the unpinned slug `deepseek/deepseek-v4.1-flash` (the bench's "default routing" row, 2.3 s p50). Default is now `deepseek/deepseek-v4.1-flash-1017`, old settings are migrated, the fastest provider is probed once per key and pinned, the HTTP client is kept for keep-alive, fallback is haiku. Measured with the real key (`cargo run --release -p lowk-llm --example ttft 8`, ~400-token prompts): legacy slug p50 1044 ms (p95 2120, n=4, jittery), shipped default **p50 855 ms, p95 1165 ms** (n=8, 550-1165). Spend well under $0.01.

## Not done / notes
* GitHub Actions run of the final commit is pending the push.
* Owner smoke test of the bundle by the orchestrator (computer use) is still required by the release gate.
* WP-C leftovers: bge-m3/e5 comparison, i5-10400F timing. Windows sidecar build is untested (compile-checked only).

## Checkpoint 2026-10-23 (tag `checkpoint-2026-10-23`, owner break ~1.5 days)
Adaptive overlay tone, verified live with the owner ("гораздо лучше", edges still imperfect):
* Bright light colours (yellow, gold, lime, light orange/green/cyan, pink) get a light tinted scrim + dark text (owner decision).
* Sampler bug fixed: SCK grabs were requested at 4:3 for a 600x384 overlay, so macOS letterboxed the bottom ~15% black and the lower tone rows were dimmed/stale ("feels like 4 zones").
* Tone grid 12x8 (was 4x3), majority-vote border cells, sharp 8 px transitions, latency ~0.3 s (100 ms poll, 2-sample confirm, 120 ms fade), 0.35 ms analysis per changed frame.
* Strict live check `just qa-tone` (border bands, latency <= 500 ms, `--grid`): last owner run interrupted after 16 FAILs, mostly at screen edges and the black/white border; latency on some quadrant cells not reached.

Open, owner said WAIT for him before starting:
1. Overlay UI took very long to appear in the live run (frame visible, content late). Measure qa (debug) vs release bundle and 96-cell render cost first.
2. Tone change ~2x faster (~0.15 s): evaluate a continuous SCStream instead of polled SCScreenshotManager grabs; ship only if CPU is not worse.
3. Settings toggle for the adaptive gradient, default OFF (plain scrim + transparency slider); off = no sampling at all (no CPU, no Screen Recording need for this feature). The gradient is mainly for overlays shown in smart glasses.
Also open: edge-of-screen tone accuracy (see the FAIL list of the last `just qa-tone`), 2 moderate Dependabot alerts, owner live check of hold-⌃, roadmap question batch (Apple Developer, Win10, camera, sync, disguise, Jev key, order).
