# M2 independent QA review (2026-10-22, HEAD 9bada33 + review commit)

Reviewer did not write the code. No sound, no computer use, no screen takeover: bundle headless modes, Playwright on the mock UI, file fixtures, a network sandbox.

## Gate results
* `just ci` (lint, ci-cross, cargo test, ui test, build, size): green locally.
* GitHub Actions for 9bada33 was in progress at review start; the final run is checked after the push (see the end of this file).
* Bundle: `just build` OK (ad-hoc, Identifier dev.lowk.app, 50 MB: app 20.9 + stt 8.1 + embed 20.7). `just qa-bundle --silent --no-build`: 65 pass, 0 fail, 1 NEEDS-GRANT (System Audio, owner action), 1 SKIP (real key presses, silent mode).
* `lowk --selfcheck-m2` with the real EmbeddingGemma file and OCR, inside `sandbox-exec` that kills the process on any outbound :80/:443 connection: 38 checks, 37 pass, 0 fail, 1 skip (live ask: no key given). The process was never killed, i.e. no cloud connection was attempted by ingest, OCR, embedding, search, local-only Ask/summary/tags, export/import.
* Playwright (chromium + webkit): m2.spec, known-bugs, main-features: 72 passed.

## Test-weakening audit (04ec933..HEAD, ui/e2e, tools/qa)
No assertion was removed or loosened to hide a failure. Changes are (a) selector migration from the old top bar to the variant A sidebar (`.tb`/`.nc` to `.side .nc`/`.np`, `.chip b` to `.tree .f .n`), (b) the G5 test was rewritten to the new rule "one New call button, always the same popover" (stricter: count 1 on every screen), (c) `.gear`/`.wm` removed from the grid-size lint because those elements no longer exist, (d) library drag test scroll offsets 300 to 160 / `scrollIntoViewIfNeeded` to a fixed scrollTop (layout change), (e) four animated/streaming states excluded from pixel baselines (recording, summary-stream, tour-try-ask, tour-try-hold) but still layout-linted; baselines were re-recorded for the new layout (they are local, gitignored: they prove nothing across machines). qa-bundle only gained checks. Minor: because baselines are re-recorded per machine, pixel regressions are only caught relative to this layout.

## Exit criteria (docs/M2-STATUS.md)
| # | Criterion | Verdict | Evidence |
|---|---|---|---|
| 1 | Search p95 < 200 ms at 100k chunks; index rebuildable | PASS (by WP-C bench, not re-run) | selfcheck "index rebuilds from the vault, same chunk ids" passes in the bundle; bench figure taken from DECISIONS-M2 (fake embedder; +25 ms real query embed) |
| 2 | Every file converts or fails with a shown error | PARTIAL | Synthetic 16-file corpus + all bundle ingests pass (PDF, XLSX, DOCX, PPTX, CSV, MD, scanned PDF via Vision). The real RU/EN corpus that MANIFEST.tsv asks for was never added |
| 3 | Hybrid beats keyword | PASS | bundle: hybrid finds the PDF by meaning with the sidecar; eval MRR@5 0.93 vs 0.50 (102 queries on 46 passages, a small haystack) |
| 4 | retrieve_pack budgets + cloud policy | PASS | selfcheck: sources before first delta, template/instructions/chunk in prompt, local-only withheld; index budget test |
| 5 | Summary + tags | PASS | summary written into `## Summary`; headings Главное/Решения/Задачи/Вопросы ко мне (+ Мой ответ), EN equivalents; tag suggestions accept/reject in UI test; live OpenRouter ask streamed 5 deltas with sources |
| 6 | UI variant A, RU+EN | PASS | Screenshots RU/EN match ux-v4 variant A: sidebar New call + Home + Search + Folders tree + Unsorted + Trash + Settings/?; folder tabs Calls/Documents/Instructions. Tour: 6 steps, Esc skips, replay from "?" and Settings (e2e RU+EN) |
| 7 | Release gate | PARTIAL | local ci + bundle QA green; GitHub run: see end. Orchestrator's own computer-use smoke of the bundle still required |
| 8 | Owner-bug regressions | PASS (none reported) | |

## Adversarial cases
| Case | Verdict | Evidence |
|---|---|---|
| Scanned PDF (OCR) | PASS | bundle: `scanned.pdf` converter pdf-ocr, 2 pages, searchable; Vision RU+EN test |
| 200-slide PPTX, 10k-row XLSX | PASS | new tests `pptx_200_slides_with_progress`, `big_sheet_10000_rows` (about 0.5 s total, progress callbacks, all slide markers in order, rows not silently lost). Conversion runs on 2 worker threads, progress is throttled to 100 ms, so no UI freeze by construction; not observed in a live window |
| Corrupt / encrypted PDF, broken DOCX | PASS | corpus test: DocEncrypted / IngestFailed / DocEmpty, status Failed with error, never an empty doc; UI e2e shows readable error |
| Duplicate upload | PARTIAL | the same file twice creates two documents ("text", "text") with identical content hash. No dedup and no warning; search then returns both. Low severity |
| Doc deleted in Finder | PASS | new test `duplicate_external_delete_and_stemming`: both files removed, rescan reports Removed, the index stops serving them, doc list updated |
| Search RU/EN/code-switch | PASS (spot check) | same test: "договоры/товары" stemming finds the PDF, "квазар supply" finds a mixed-language hit, nonsense query returns no hits. Caveat: in a tiny corpus (under 5 chunks) every stem is "very common" and dropped from the FTS query, so a 2-doc vault returns nothing for ordinary words (the semantic side still works). Real vaults are unaffected |
| Ask with sources, chips open doc / call minute | PARTIAL | verified in mock UI (overlay Answer chips, Sources tab, palette opens the call at the minute, viewer opens at the page). The overlay chip to real main window hop needs a real window |
| Pinned doc always included, size-limited | PASS | lowk-index test `budgets_pack_within_limits_and_report_dropped_pinned`; UI budget meter and presets |
| Local-only: zero cloud calls | PASS | see sandbox run above; also selfcheck "sends nothing to the cloud", cloud embedder skips the folder, summary/tags refused |
| Export to import zip round-trip | PASS | selfcheck: 16 files, 7 documents back; UI shows result, bad archive is an error |

## Issues
* Medium: criterion 2 corpus is synthetic only (no real RU/EN documents were ever run).
* Low: duplicate upload not detected (consider a "same file already here" prompt using the stored sha).
* Low: live ask TTFT in the bundle self-check was 3.2 s (includes first connection and the one-time provider probe), above the 855 ms p50 claimed; claim was measured warm via the example. Worth a warm vs cold note.
* Low: FTS very-common-stem rule makes tiny vaults return nothing for ordinary words.
* Low: svelte `assignment_value_stale` warnings in App.svelte and EmbedderCard.svelte (fixed in this review).
* Info: pixel baselines are local and re-recorded; `ci` does not run the Playwright visual suite (`just test-ui-visual` is separate).

## Owner should try by hand
1. Hear it: start a call with sound, press the Ask bind, check the answer shows source chips; click a chip and confirm the main window opens that document page or the earlier call at the right minute (needs the real overlay and click-through mode).
2. Drop a real PDF scan, a big deck and a big spreadsheet into a folder in the real window; watch that the window stays responsive and the progress and cancel are clear.
3. Delete a document in Finder while the app is open; confirm it disappears from the folder and from search.
4. Mark a folder "local only", connect Ollama or LM Studio, then press Ask and Summary: check it answers locally; without a local model the buttons must be disabled with an explanation. (Verify with Little Snitch or Activity Monitor if you want proof.)
5. Run the tour from "?" and from Settings in RU and EN; skip it with Esc.
6. Grant System Audio to the bundle (NEEDS-GRANT) and repeat the headphones check.
7. Upload the same file twice and decide whether duplicates should be blocked.
