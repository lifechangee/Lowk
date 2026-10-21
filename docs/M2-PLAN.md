# M2 plan: context store, search, ingest, summary, new main window

Status: contracts landed in `lowk-types` (this commit). Four work packages can start in parallel; one integration package follows.
Product decisions: `BRIEF.md` section "M2 — решения". Architecture: `ARCHITECTURE.md` sections 1, 4, 5. Research: `research/context-store.md`. UX: `design/ux-audit.md`, `design/ux-v4.html` (variant A + tour).

## 1. Scope

| # | Feature | Decision / detail |
|---|---|---|
| 1 | **Document ingest** | PDF (text and scanned), DOCX, PPTX, Keynote, XLSX, CSV/TSV, TXT, MD, images (OCR). Original is kept untouched next to a converted `.md` (`type: doc`, `source_sha256`, `converter`, page markers `<!-- p:N -->`; slides and sheets use the same marker with `unit: slide\|sheet` in frontmatter). Big files are loaded whole, only relevant chunks reach the model; a "this will take a while" warning above `docs.warn_slow_mb`. No silent fallbacks: failure = `Failed` status with an error code, never an empty document; findings that do not fail are `IngestWarning`s shown on the document. |
| 2 | **Pinned documents** | Star in the folder's Documents tab. Pinned docs of the folder always go to the model, capped by `docs.pinned_budget_tokens` (default 6000; the meter and "N did not fit" are shown). |
| 3 | **Folder instructions** | Editor in the folder's Instructions tab (about 500 tokens, counter, example text). Always sent. Stored in `_folder.md` `## Instructions` (already supported by the store). |
| 4 | **Hybrid search** | One line over calls, documents and screenshots (caption/OCR text). FTS5 over stemmed RU+EN text, `sqlite-vec` vectors from EmbeddingGemma, RRF fusion, boosts (same folder, recency, doc vs speech, penalty for `qa`). Filters folder / tag / date. Click opens the call at the minute (`SourceRef.at_s`) or the document at the page. While vectors are missing or building the result says `mode_used = keyword` and the UI shows it (no silent downgrade). |
| 5 | **Retrieval for Ask** | `Retriever::retrieve_pack`: folder instructions + pinned docs + relevant chunks of documents + relevant chunks of **previous calls of the same folder** (the current call's transcript is excluded, it is the live delta). Cloud policy (`allow_cloud=false`) withholds chunks and reports `withheld`. Budgets from `RetrievalQuery`. M2 delivers the retriever; the Ask flow in `lowk-session` adopts it in M3. |
| 6 | **Sources under answers** | `LlmEvent::Sources { req_id, sources, withheld }` (`llm://sources`) right after `Start`; the overlay and call page show labels (document p. N, call mm:ss); click opens the source in the main window. The plumbing is built and tested in M2 (fake LLM); M3 connects it to real Ask. |
| 7 | **Summary and tags (cloud LLM)** | Button after a call: 3-5 key points, decisions, tasks (who/what), questions to me and how I answered. Streams over `summary://event`, written into `## Summary`. After the summary, AI suggests tags (prefers the vault's existing vocabulary); the user confirms; manual tags stay. Model `deepseek/deepseek-v4.1-flash-1017`, provider pinned, reasoning off (`llm.summary_model`, `llm.pinned_provider`). BYOK key in the OS keychain (`secret_set("openrouter", ..)`, already exists). |
| 8 | **Embedder** | EmbeddingGemma 2 Q8_0 GGUF (~310 MB) downloaded with the models on demand (`embedder_install`, progress as `engine://progress` with `engine == "embedder"`), checksum pinned, licence shown first. Cloud embeddings are an option (`embedder.mode = cloud`), `off` = keyword only. Model change = background rebuild, FTS keeps working. |
| 9 | **Export / import** | Whole vault and single folder as zip; the vault opens in Obsidian as is. `export_vault` / `import_vault` exist; M2 adds `folder_export` / `folder_import` and zip support, ids reassigned on collision, never overwrites. |
| 10 | **Main window IA, variant A** | One permanent sidebar (logo, New call, Home, Search, Folders tree, Unsorted, Trash, Settings, recording indicator); folder = tabs Calls / Documents / Instructions; call = tabs Transcript / Summary / Questions to AI / Screenshots / Sources, actions in the header menu; breadcrumbs; search palette (Cmd+K) plus full results page with filters; empty states from `ux-audit.md`. Plus the first-run **tour** (6 steps, skippable, repeat from Settings and "?"; try-it demos never touch the network and are labelled as examples). `onboarding.tour_done`. |

Out of scope (later): code files as documents, chat with AI over a call, diarization, sync, encryption, folder-level cloud embeddings choice per folder.

## 2. Exit criteria (all required)

1. Search p95 < 200 ms at 100k chunks (benchmark in `lowk-index`, CI-sized variant plus a documented full run). Index fully rebuildable from the vault (delete `.lowk/index.db`, relaunch, same results).
2. Ingest corpus: 30-50 real RU/EN files across all kinds (PDF text + scanned, DOCX, PPTX, Keynote on macOS, XLSX, CSV, images). Every file either converts with correct page markers or fails with a shown error; zero silent empties. 300-page PDF and a 5000-row sheet converted with progress and without UI freeze.
3. Hybrid beats keyword on the RU/EN code-switch eval (100-200 queries); eval and embedder choice recorded in `DECISIONS-M2.md` (fallback model if EmbeddingGemma 2 loses: bge-m3 / multilingual-e5-small).
4. `retrieve_pack` returns instructions + pinned + relevant + previous-call chunks within the budgets; cloud policy test; byte-stable chunk ids across reindex.
5. Summary and tag suggestions work against OpenRouter with the pinned DeepSeek model: a golden-prompt test with `FakeLlm`, plus one live run (owner key) recorded; errors (no key, offline, 4xx/5xx, cancel) surface as `ErrorCode`s in the UI.
6. UI: variant A in RU and EN, no overflow/lint failures (`just test-ui-visual`), tour passes in both languages, every new state registered in `ui/e2e/lib/states.ts`.
7. Release gate (CLAUDE.md): `just ci` incl. `ci-cross` + green GitHub Actions; `just qa-bundle` green on the bundled app from a clean profile with new checks: import each document kind, search finds it, summary against a stub server, export/import round trip; orchestrator smoke test of the bundle covering all previously working features. No demo/fake data in user builds (fakes live only in tests and the `qa` feature).
8. Every owner-reported bug gets a permanent regression check against the bundle.

## 3. Contracts (landed)

All in `crates/lowk-types` (architect-gated; additive change requests go to the architect, do not edit it from a WP).

* `docs.rs`: `DocKind`, `IngestStatus`, `IngestWarning(Code)`, `Document`, `NewDocument`, `DocPatch`, `IngestRequest`, `IngestedDoc`, `IngestProgress`, `PinnedUsage`; traits `DocIngestor` (lowk-ingest), `DocumentStore` (lowk-store).
* `search.rs`: `SourceRef` / `RefKind`, `SearchQuery` / `SearchFilters` / `SearchMode` / `SearchHit` / `SearchResults`, `IndexItem` / `IndexFolder` / `IndexStatus` / `SemanticState`, `EmbedderInfo`, `ContextPack`, `ReindexScope`; trait `SearchIndex` (lowk-index). `Retriever::retrieve_pack` (default wraps `retrieve`).
* `llm.rs`: `ContextChunk` now carries `source_ref`, `pinned`, `tokens`; `RetrievalQuery` gains `pinned_budget_tokens`, `include_folder_context`, `include_previous_calls`; `LlmEvent::Sources`; `Summary`, `SummarizeInput`, `QaDigest`, `SummaryEvent`, `TagInput`, `TagSuggestion`, `LlmProviderStatus`; trait `LlmService` (lowk-llm).
* `settings.rs`: `Settings.embedder` (`EmbedderMode` local/cloud/off, model ids), `Settings.docs` (pinned budget, slow-file warning, max size, OCR, retrieved budget), `LlmSettings.summary_model` / `pinned_provider`, `OnboardingSettings.tour_done`, constants `DEFAULT_LLM_MODEL_PINNED`, `DEFAULT_EMBEDDER_ID`.
* `error.rs`: `DocUnsupported`, `DocEncrypted`, `DocTooLarge`, `DocEmpty`, `OcrFailed`, `IngestCancelled`, `EmbedderMissing`, `EmbedFailed`, `LlmBadResponse`, `ArchiveInvalid` (plus existing `IngestFailed`, `Index`, `ApiKeyMissing`, `Network`, `LlmProvider`). UI strings live in `ui/src/lib/i18n/*/errors.ts` (WP-A).
* `event.rs` / `events`: `AppEvent::{Ingest, Index, Summary}`; names below.
* `fakes.rs`: `FakeDocumentStore`, `FakeIngestor`, `FakeSearchIndex`, `FakeLlmService`, `FakeRetriever`, `fake_source`.

### Tauri commands (stubs in `crates/lowk-app/src/commands/{docs,search,ai,transfer}.rs` return `NotImplemented`; typed bindings regenerated; permissions in `build.rs` + `capabilities/main.json`)

| File | Commands |
|---|---|
| `docs.rs` | `doc_list(folder)`, `doc_get(id)`, `doc_import(files)`, `doc_get_md(id)`, `doc_save_md(id, md)`, `doc_patch(id, patch)`, `doc_delete(id)`, `doc_restore(id)`, `doc_retry(id)`, `doc_cancel(id)`, `docs_pinned_usage(folder)` |
| `search.rs` | `search_query(query)`, `index_status()`, `index_rebuild(scope)`, `embedder_info()`, `embedder_install()`, `embedder_install_cancel()` |
| `ai.rs` | `llm_summarize(call)`, `llm_summarize_cancel(call)`, `llm_suggest_tags(call)`, `llm_provider_status()`, `llm_key_test()` |
| `transfer.rs` | `folder_export(id, dest)`, `folder_import(src)` |
| existing | `folder_*`, `call_*` (`call_set_tags` confirms tags), `export_vault`, `import_vault`, `secret_*`, `settings_*`, `llm_ask`, `llm_cancel` |

### Events (backend to UI)

| Event | Payload | Window |
|---|---|---|
| `doc://progress` | `IngestProgress` | main |
| `index://status` | `IndexStatus` | main |
| `summary://event` | `SummaryEvent` (`start`/`delta`/`done`/`error`) | main |
| `llm://sources` | `{ type: "sources", req_id, sources: SourceRef[], withheld }` | overlay + main |
| `engine://progress` | existing `EngineProgress`, `engine == "embedder"` | main |
| `store://changed` | existing; documents emit `is_folder: false` | main |

## 4. Work packages (parallel; disjoint file ownership)

Rules for all: work in a branch/worktree, touch only your paths, depend only on `lowk-types` (and `lowk-platform` where noted), never on each other's crates. Append dependency lines to `Cargo.toml` `[workspace.dependencies]` under a `# M2 WP-x` comment (append-only; resolve `Cargo.lock` by rebase + `cargo update -w`). `cargo deny` licences stay MIT/Apache/BSD/MPL. Each WP ships tests, fixtures and the fakes it needs, and keeps `just ci` (incl. `ci-cross`) green on its own. No unsafe without a comment. Windows code compiles through `ci-cross`.

### WP-A: UI (main window)

Owns: `ui/src/main/**`, `ui/src/lib/components/**`, `ui/src/lib/i18n/{ru,en}/**` (new namespaces `docs`, `search`, `tour`, `summary`; extends `main`, `settings`, `errors`), `ui/e2e/**` states and specs, mock IPC `ui/src/main/api/{mock,fixtures,types}.ts`. Does not touch `ui/src/overlay/**` (M3 overlay sources UI is a small follow-up: the overlay only needs `llm://sources`, agree with WP-E).

Deliver:
1. Variant A shell: persistent sidebar, breadcrumbs, one New-call popover, settings as a sidebar section with top tabs, empty states (texts from `ux-audit.md`).
2. Folder workspace tabs Calls / Documents / Instructions: document list with status chip (`IngestStatus`), progress bar from `doc://progress`, warnings and error with retry/cancel, star (pin) + budget meter (`PinnedUsage`), drag-and-drop and "Add document" (file dialog), slow-file warning, document viewer + Markdown editor (CodeMirror lazy chunk), instructions editor with token counter.
3. Search: Cmd+K palette and full results page; filters folder/tag/date; grouped hits with highlights (`SearchHit.highlights`, UTF-16 offsets); "keyword only" notice when `mode_used != hybrid`; click navigates to call at `at_s` or document at `page`.
4. Call page: Summary tab (button, streaming, tasks/decisions/questions, regenerate), tag suggestions chips (accept/reject, then `call_set_tags`), Sources tab, source labels component reusable by the overlay.
5. Settings: embedder card (install/progress/off/cloud, size and licence), LLM provider card (key field via `secret_set`, `llm_key_test`, model, pinned provider), documents card (pinned budget, OCR), export/import (vault and folder).
6. First-run tour: 6 steps, skippable (button + Esc), progress "2 / 6", overlay thumbnail with labelled demo, replay from Settings and "?"; `onboarding.tour_done`.
7. Everything RU+EN with key-parity test; mock IPC implements all new commands using the contract fakes' shapes; QA states registered; webkit + chromium visual suite green.

Done when: the UI runs end to end against mock IPC (including failure states) and the visual/lint suite is green.

### WP-B: ingest (`lowk-ingest` + document storage in `lowk-store`)

Owns: `crates/lowk-ingest/**`, `crates/lowk-store/src/docs.rs` (new), `crates/lowk-store/src/{doc_codec.rs}` (new), minimal additive hooks in `crates/lowk-store/src/{lib,vault}.rs` and `transfer.rs` for folder zip export/import. Fixtures in `fixtures/ingest/**`.

Deliver:
1. `lowk-ingest`: `impl DocIngestor`. PDF: text extraction with layout cleanup (headers/footers, hyphenation, columns), scanned detection per page, OCR (macOS Vision through the platform bindings, Windows `Windows.Media.Ocr`; no Tesseract bundle; OCR via a trait so tests use a stub), page markers; DOCX (headings, lists, tables to Markdown, footnotes); PPTX (slide text, notes, tables, `<!-- p:N -->` per slide); Keynote (macOS: Keynote automation export to PDF then PDF path; other cases fail with `DocUnsupported` and a "export to PDF/PPTX" param, never silently empty); XLSX/CSV (per sheet tables, row/column caps with `SheetTruncated`, number/date formatting preserved); TXT/MD (encoding detection, frontmatter kept); images (OCR, plus dimensions). Progress and cancellation, memory-bounded streaming for big files, no panics on malformed input (fuzz/corpus tests).
2. `lowk-store`: `impl DocumentStore` over the vault: `Docs/` layout from ARCHITECTURE 4.3, `type: doc` frontmatter, atomic writes, sanitized names, trash/restore with the original, round-trip tests with golden files, watcher events for externally added files (treated as imports, status `Queued`), `PinnedUsage` helper (token estimate), folder-level zip export/import incl. documents and Obsidian-friendly layout.
3. Corpus test harness: `cargo test -p lowk-ingest --features corpus` over `fixtures/ingest` with a manifest of expected page counts and key phrases.

Done when: the corpus passes and `DocumentStore` golden tests pass on Linux CI (OCR/Keynote paths are macOS/Windows-gated tests plus stubs).

### WP-C: index and search (`lowk-index` + embedder)

Owns: `crates/lowk-index/**`, embedder download/runtime code inside it (`src/embedder/**`), optional sidecar under `workers/lowk-embed-worker/` (not a workspace member, same pattern as `lowk-stt-worker`), `tools/index-bench/**`, `fixtures/index/**`. May reuse the download/checksum helper pattern of `lowk-stt::models` by copying it (crates do not depend on each other).

Deliver:
1. SQLite schema + migrations (`meta`, `files`, `chunks`, `chunks_fts` stemmed RU+EN, `chunks_vec` via `sqlite-vec` behind `VectorIndex` with a flat f16 fallback), chunker (transcript windows 200-300 tokens on speaker/pause boundaries, never across `lowk-image` / `lowk-qa` blocks; docs by headings 300-500 tokens with `heading_path` and page from markers; `text_hash` skip), `impl SearchIndex` (`upsert`, `upsert_folder`, `known`, `reindex`, `remove`, `status`) and hybrid `search` (FTS top-50 + vector top-50, RRF k=60, boosts, filters, facets, highlights as UTF-16 ranges, neighbour merge).
2. `impl Retriever` incl. `retrieve_pack`: instructions, pinned within budget, relevant docs, previous calls of the folder, `exclude` ids, cloud policy and `withheld`, budget packing, `SourceRef` on every chunk. Warm-up API for speculative retrieval.
3. Embedder: `trait Embedder` implementation for EmbeddingGemma 2 Q8_0 (llama.cpp via sidecar or crate: choose after the 1-day eval below), download with SHA-256 pin and resume, `EmbedderInfo`, progress as `EngineProgress` with `engine == "embedder"`, background embedding queue (low priority, yields to live STT), model change rebuild, `SemanticState`, cloud embedder over OpenAI-compatible `/embeddings` (key passed in by the app), `off` mode.
4. Eval + benchmark: 100-200 RU/EN code-switch queries over synthetic transcripts/docs: EmbeddingGemma vs bge-m3 vs multilingual-e5-small (recall@5, speed on M4 and i5-10400F); search benchmark at 100k chunks (p95 < 200 ms). Record in `docs/DECISIONS-M2.md`.

Done when: search/retrieval tests pass on Linux CI with a deterministic fake embedder, the eval and benchmark are recorded, and the real embedder installs and embeds on the Mac.

### WP-D: LLM (`lowk-llm`)

Owns: `crates/lowk-llm/**`, `fixtures/llm/**`.

Deliver:
1. OpenRouter streaming client (OpenAI-compatible SSE; own tolerant SSE parser, cancellation by drop, timeouts, retries only before the first token), `ProviderProfile` for OpenRouter (pinned version slug, `provider.order` + `allow_fallbacks:false` for a pinned provider, `reasoning.enabled=false`, `data_collection:"deny"` toggle, stable `session_id`), key passed in by the caller (never read from the keychain here, never logged), error mapping to `ErrorCode` (`ApiKeyMissing`, `Network`, `LlmProvider`, `LlmCancelled`, `LlmBadResponse`).
2. `impl LlmService`: `summarize` (golden-tested prompts RU/EN, map-reduce over a long transcript, strict structured output: key points, decisions, tasks, questions + my answers, Markdown `md` for `## Summary`, answers in the call's dominant language), `suggest_tags` (vocabulary-first, normalisation, max N, dedupe).
3. Latency probe helper: pick and return the fastest upstream provider for the pinned model (for `llm.pinned_provider`), measured as in `research/llm-and-jev.md`.
4. M3 readiness: `PromptBuilder` input accepts `ContextPack` and emits `LlmEvent::Sources` data (deduplicated `SourceRef`s, stable order, `withheld`), cache-friendly append-only layout kept (`ChatLedger` byte-stability test), `ChatClient::stream` signature extended through a new trait method rather than breaking `FakeLlm`.
5. Offline test server (wiremock-style, in-crate) used by `just qa-bundle` through a base-URL override; one opt-in live test behind an env var.

Done when: unit/golden tests and the SSE fuzz cases pass offline, and one live run against OpenRouter is recorded (summary + tags on a sample call).

### WP-E: integration (one agent, after A-D land)

Owns: `crates/lowk-app/**` (commands, state, boot, emitter, qa), `crates/lowk-session/**` changes needed for sources and retrieval, `tools/qa/**`, `docs/QA.md` additions. Adds `lowk-index`, `lowk-ingest`, `lowk-llm` as `lowk-app` dependencies.

Deliver:
1. Replace each `NotImplemented` stub with the real glue (thin; logic stays in the crates): ingest queue with a small worker pool (import, convert, `save_converted`, index, status events, cancel/retry, crash recovery of `Converting` documents on start), `IndexItem` feeding from the vault (startup reconcile against `SearchIndex::known`, `store://changed` driven upserts, trash handling), embedder settings and install, summary + tags commands writing `## Summary` and emitting `summary://event`, provider status/key test, folder export/import, `settings` effects (embedder mode switch, pinned budget).
2. Session: Ask uses `retrieve_pack` with folder context and previous calls, emits `LlmEvent::Sources` before the first delta and passes the pack to the prompt builder (this is the seam M3 fills; if M3 is not started, keep the behaviour behind the template path already used by `llm_ask` and verify with `FakeLlm`).
3. Overlay: render `llm://sources` labels (small change in `ui/src/overlay/**`, coordinated with the WP-A owner after WP-A finishes).
4. QA: new checks in `tools/qa/bundle-qa.mjs` and a `qa` feature test channel for import, search, summary with a stub server, export/import round trip; permanent regression checks for any owner bug; `just qa-bundle` green; orchestrator smoke test of the bundle (computer use) of every previously working feature, then the owner build.
5. Docs: `DECISIONS-M2.md` (eval results, embedder choice, ingest limits), QA.md, README feature list.

## 5. Dependencies and order

```
          contracts (done)
   ┌────────┬────────┬────────┐
  WP-A     WP-B     WP-C     WP-D      (parallel; each is testable alone through the Fake* in lowk-types)
   └────────┴────┬───┴────────┘
               WP-E  integration, QA, bundle gate
```

* WP-A is unblocked by the generated bindings and the mock IPC; WP-B/C/D by the trait definitions and fakes.
* Contract changes: file a short note to the architect; additive only (new optional fields/variants with `#[serde(default)]`), regenerate bindings in the same commit.
* Shared files: `Cargo.toml` (append-only per WP block), `crates/lowk-app/src/commands/mod.rs` and `build.rs` (WP-E only from now on), `ui/src/lib/ipc/bindings.ts` (generated, never hand-edited).

## 6. Risks

| Risk | Mitigation |
|---|---|
| EmbeddingGemma 2 is a week old; llama.cpp support for `gemma-embedding2` | Eval first (WP-C day 1); `Embedder` trait + rebuildable index; fallback v1 Q4 / bge-m3 / e5-small; FTS-first means search ships regardless. |
| OCR quality and platform differences (Vision vs Windows OCR) | OCR behind a trait, warnings (`OcrLowConfidence`), RU/EN fixtures per platform; Windows verified by compile only until the PC is available. |
| Keynote has no open parser | macOS automation to PDF, clear error and "export to PDF/PPTX" hint elsewhere; never silent. |
| Large files freeze or exhaust memory | Streaming conversion, bounded queues, progress and cancel, caps with warnings, 300-page and 5000-row tests. |
| Indexing steals CPU from live STT | Background queue at low priority, paused while a session records on weak hardware (`HwInfo`), measured in the soak. |
| Index drift after sleep/sync/external edits | Start-up reconcile by `blake3`, watcher with debounce, full rebuild command. |
| Pinned docs blow the prompt | Hard token budget with visible meter and `pinned_dropped`; delta is never cut for retrieved/pinned context (ARCHITECTURE 5.2). |
| UI scope (variant A is a reshell of the main window) | WP-A owns it alone; visual/lint suite from QA.md is the gate; mock IPC keeps it independent of backend timing. |

## M2 follow-ups
- WP-D live TTFT 2.3 s vs 1.2 s in spikes/llm-bench (provider selection; one of four providers returned HTTP 400) — re-check pinning/provider order during WP-E.
