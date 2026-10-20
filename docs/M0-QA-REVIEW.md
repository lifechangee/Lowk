# M0 independent QA review (2026-10-20)

Reviewer ran: `just ci` (green), `just qa` with `LOWK_QA_MULTI=1` (528 browser tests chromium+webkit: 525 passed, 3 skipped; real-app 17/17; gallery 236 cards, 0 failing), `cargo check --workspace --target x86_64-pc-windows-msvc` (green, dummy llvm-rc). Stale local pixel baselines (gitignored) were re-recorded first; the 54 diffs before that were the intended onboarding / appearance changes.

## Test audit (f3a438e..HEAD, ui/e2e/**, tools/qa/**)
* `lint.ts` / `contrast.ts`: three narrowings: ellipsized text measured only over its own box, text clipped by scrolling ancestors skipped, and only the `[role=dialog]` measured when a modal scrim is open. All three match what is physically painted, so no real defect is hidden by them. Caveat: ellipsized (truncated) titles now pass lint by design, so N2/N6/N7 are "fixed by truncation", not by layout; review long titles by eye.
* `known.ts`: all KNOWN entries removed and suite is clean; xfail lines removed for F1 F2 F11 F13 F12. No rule was loosened beyond the above.
* **Weakness found and fixed**: the real-app F6 check read `getComputedStyle(body).fontSize`, which CSS `zoom` (the F6 implementation) never changes, so it reported "CAUGHT" (false negative). It now measures the rendered h1 height (36px -> 46px at font_px 18). `fid:` flags for F1/F2/F6 were still set in app-qa.mjs (made passing checks report UNEXPECTED); removed.
* Added real-app check F8 (overlay stop button -> main window visible on `#/`). F5 split-backdrop run is kept as `docs/qa/review/f5-real-split.mjs` + `f5-split-backdrop.swift` (manual, not in `just qa`).

## F-id table
| ID | Status | Evidence | Notes |
|---|---|---|---|
| F1 | PASS | docs/qa/real/main-onboarding-0.png, shots/chromium/main-onboarding-2-en-S.png | Full-bleed in real WKWebView (1100x688 at 0,0). |
| F2 | PASS | docs/qa/real/main-home.png | Dot/label/kbd spaced; real + browser lint clean on all routes. |
| F3 | PASS | shots/webkit/overlay-moving-light-t60-en-L.png | Move badge in-flow in footer, fully inside. Not checked live with a real held key. |
| F4 | PASS | docs/qa/real/f4-sample-last.png, results.json | 20 flips, 121 Hz presence sampler, 0 absent, 622 stream frames, 0 blank. |
| F5 | PASS | docs/qa/review/f5-split-t60.png, f5-split-t90.png | REAL app, real SCK sampler over a black/white split window: 4x3 grid reports dark,dark,light,light; answer block switches to dark text on light scrim, header/footer stay light. Smooth gradient. |
| F6 | PASS | results.json | Overlay 14->18px; main zoom 1.29x (h1 36->46px). Note: main uses CSS zoom (scales everything, incl. icons). |
| F7 | PASS (unit/browser) | ui/e2e/main-features.spec.ts | Panic unbound; not re-tested with real hotkeys. |
| F8 | PASS | results.json (new check) | Real app: stop click -> main shown on `#/`. |
| F9 | PARTIAL | shots/chromium/main-home-drag-ru-L.png | Drag, drop highlight, edge autoscroll, move menu PASS in chromium+webkit (mock). `call_move` backend exists but real-app drag not testable (no call seeding in qa channel). |
| F10 | PASS | results.json | Real DOM, 78 snapshots, no duplicate words. |
| F11 | PASS | shots/webkit/overlay-passive-*-t90-*.png | Text AA over checkerboard at 0/60/90. Subjectively, at 90 on light the page behind still ghosts through between lines (readable, busy). |
| F12 | PASS | known-bugs.spec.ts | Onboarding preselect = home default in both mock and no-app-list real case. |
| F13 | PASS | overlay-toast-color-t60-ru-L.png | Toast and «Мой ответ» in-flow, no overlap. |
| F14 | PASS | crates/lowk-session/src/qa.rs | Only NextAsk and SessionEnd close it. |
| N1-N7 | PASS | clean lint | See audit caveat for N2/N6/N7. |

## Remaining issues
* Minor: the overlay stop button is an 8px red glyph on a faint chip, easy to miss (enlarged to 10px in this review; consider a brighter chip).
* Minor: opacity 90 on light/busy backdrops shows backdrop text through the answer area.
* Minor: tone sampler needs Screen Recording; without it the overlay stays on the global tone.
* Gap: Windows tone sampler is a stub; Windows not run.
* Gap: F9 real-app drag, F3 live key-hold, F7 real global hotkeys are untested by machines.

## Owner should try by hand
1. Hold ⌥ over a split dark/light window: badge visible, overlay follows, mode restores.
2. Start a call, press stop in the overlay header: main opens on the library with the new call on top.
3. Drag a call from Unsorted onto a folder card and onto the tree; also right-click -> Move to.
4. Settings -> text size 12 and 20: main and overlay both rescale; check long RU/EN strings.
5. Overlay at opacity 90 over Discord/browser with busy text; flip Interactive/Passive repeatedly (no blink).
6. Fresh onboarding: pick "Whole system" and "One app", confirm home source matches.
