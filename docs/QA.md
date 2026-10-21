# QA: how machines check the UI before a human sees it

Owner feedback ([M0-FEEDBACK.md](M0-FEEDBACK.md)) showed that unit tests do not catch visual and overlay
behaviour bugs. This is the machinery that does. **Every UI or overlay fix must be verified with it.**

## What exists

| Layer | Command | What it checks |
|---|---|---|
| UI visual + layout suite | `just test-ui-visual` | Playwright, **chromium and webkit** (the app is WKWebView), against the mock IPC (`vite` on port 1431, never 1420). Every main-window route and state, every settings page, all 8 onboarding steps, search palette, overlay passive / interactive+template menu / moving / pill / toast, 3 backgrounds x opacity 0/60/90, RU and EN, 2 sizes. Per state: pixel baseline + layout lint (text clipping/overflow, text and sibling overlap, touching chips = missing gaps, 4px grid, pixel-based WCAG contrast with text hidden, outside-viewport). |
| Owner-bug detectors | `ui/e2e/known-bugs.spec.ts` (part of the above) | One test per F-id the browser can see. Expected-fail (`test.fail`) until fixed. |
| Real-app QA | `just qa-app` (macOS) | A `qa` build of the real app driven over a localhost token channel. Lints the real WKWebView DOM, captures the real windows (including the overlay) through ScreenCaptureKit, samples the overlay at >=100 Hz across mode switches, checks streamed-answer text for duplicated words from the real DOM. |
| Gallery | `just qa-report` -> `docs/qa/report.html` | Every screenshot (chromium, webkit, real app) next to the matching v3 mockup frame, failures red, known bugs amber, F-id detector table. |
| Everything | `just qa` | guard + UI suite + real app + report. |

## Rules for fix agents

1. **Before you touch anything**: `just test-ui-visual-update` (records local pixel baselines from main; baselines are gitignored because font rendering is machine specific) then `just qa`. Keep `docs/qa/report.html` (or its summary) as the "before".
2. Fix the bug. If your F-id has an expected-fail test, it will now **pass, which Playwright reports as a failure** ("expected to fail but passed"). That is the signal to finish the job: delete the `xfail(...)` line in `ui/e2e/known-bugs.spec.ts` (and the `fid` entry from `ui/e2e/known.ts`; for real-app checks, drop `fid:` in `tools/qa/app-qa.mjs`) so the check becomes a permanent regression test.
3. **After**: `just qa` again. Required: no new failures versus "before" (red cards in the report), your F-id flipped from "caught" to passing, `just ci` green. If your change intentionally alters the look, run `just test-ui-visual-update`, **look at the new screenshots in the report next to the v3 mockup**, and say so in the PR.
4. Attach `docs/qa/report.html` (or the F-id table and the paths of the screenshots that changed) to the PR / hand-off message. Never merge a UI change that nobody looked at in the gallery.
5. Debug an expected-fail test: `QA_SHOW_XFAIL=1 pnpm --dir ui exec playwright test known-bugs` runs them as normal tests and prints the real failure.
6. New UI, new state: add it to `ui/e2e/lib/states.ts` (main routes / overlay states). New product bug found by lint that you cannot fix now: add a `KNOWN` entry in `ui/e2e/known.ts` with an `N<n>` id and a comment; never loosen a lint rule to make something pass.

## Lint rules (`ui/e2e/lib/lint.ts`, `lib/contrast.ts`)

`overflow` (text clipped without ellipsis, or outside a non-scrolling clipping ancestor), `outside` (outside the viewport), `text-overlap` (text boxes of unrelated elements intersect; popovers exempt), `box-overlap` (in-flow siblings intersect), `flex-gap` (a chip - dot, icon, kbd, bordered or filled box - touches neighbouring text or chip on a line: the F2 pattern), `grid` (key containers: position/height/padding/gap not multiples of 4px), `contrast` (pixel based: text is made transparent, the page screenshotted, text colour compared against the 10th-percentile worst background pixels; below 3:1 fails, 3..4.5 is a warning in the gallery), `dup-words` (consecutive duplicated words, F10).

The same `lintInPage` runs inside the real webview in `just qa-app`.

## Real-app QA mode (`qa` cargo feature)

* `cargo build -p lowk-app --features qa,custom-protocol` (debug only). **Never in release**: `crates/lowk-app/src/lib.rs` has a `compile_error!` for `qa` without `debug_assertions`, `tools/qa/check-no-qa-release.sh` (run by `just lint`/CI, `just qa-guard`) additionally verifies that no default feature, recipe or workflow enables it and that a release build with the feature fails to compile.
* The app binds `127.0.0.1:$LOWK_QA_PORT` (random if unset), requires header `X-Lowk-Qa-Token`, rejects foreign `Host` headers, and writes `{port, token, pid}` to `$LOWK_DATA_DIR/qa.json`. `LOWK_QA_MULTI=1` disables the single-instance hand-off so a QA instance can run beside the developer's own `just dev`; it uses its own temp data dir and vault.
* Commands (POST `/cmd`, body = one line): `bind <action> <down|up>` (feeds a bind event through the real dispatcher), `hover-state` (overlay mode + native `ignoresMouseEvents`), `pill-state`, `screenshot` / `queue-png <i>` / `queue-clear` (G7 queue), `nav <hash>`, `eval <main|overlay|pill> <js expr>` (result comes back as JSON; the page parks it in its URL fragment with `history.replaceState` because the CSP forbids calling out and `eval` has no return channel), `win <label>`, `pid`, `opacity N`, `font PX`, `lang ru|en`, `mode passive|interactive|moving`, plus the whole stealth-probe set (`overlay-show`, `demo-start`, `demo-stop`, `overlay-move X Y`, `exclusion on|off`, ...).
* **Capturing the overlay**: the overlay is `NSWindowSharingNone` (stealth). ScreenCaptureKit with a per-window `desktopIndependentWindow` filter still does not deliver such a window while the exclusion is on, so the QA run sends `exclusion off` before capturing and restores it (`exclusion on`) in `finally`. Only qa builds can do that. The **presence sampler needs no lift**: it polls window-server metadata (`CGWindowList` on-screen flag and alpha) at ~120-250 Hz, which is available for protected windows. `tools/qa/qa-cap.swift` also streams the window at 60 fps through ScreenCaptureKit and counts blank frames (needs the exclusion lift).
* Needs Screen Recording permission for the terminal/host that runs `node tools/qa/app-qa.mjs`.

## Detector status (first run)

See the F-id table at the top of `docs/qa/report.html`. Browser-visible: F1, F2, F11, F12, F13 (toast). Real-app only: F4 (flicker), F6 (main text size), F10 (duplicated deltas: the browser mock does not reproduce it, the real event path does). Not detectable by machines: F3, F5 (needs judgement), F7 (decision), F8 and F9 (feature work: add tests with the feature).

## What "green" means

"Green" means **both**: (1) local `just ci` passes, which includes `just ci-cross` (clippy `-D warnings` for `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`; one-time setup: `rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-msvc`, the recipe creates the dummy `llvm-rc` itself), and (2) the GitHub Actions run for your pushed commit is green (lint, test-macos, test-windows). macOS-only `just ci` is not enough: Linux/Windows cfg-gated dead code and CRLF issues only show up there. Check with `gh run watch`.

## Round 2 checks (G1 G2 G3 G5 G6 G7)

Permanent regression tests, written together with the fixes (no expected-fail stage):

* `ui/e2e/round2.spec.ts` (chromium + webkit): G1 presets apply on click and the control keeps its size/position; overlay preview follows the overlay size; G5 no header button on home, popover elsewhere; G3 picker (running apps, presets, installed «Other…», no id field); G6 footer shows the real binds and holding Ctrl makes the overlay clickable; G7 header chip with thumbnails + clear.
* New visual states: `newcall-popover`, `source-picker`, `source-other` (main) and `hover` (overlay).
* `tools/qa/app-qa.mjs` (real app): G1 clicks the REAL settings control and reads the overlay font-size (S 12 / M 14 / XL 18) and the main zoom; G5/G3 in the real webview; G6 20x hold-Ctrl through the real dispatcher with the `ignoresMouseEvents` readback and the >=30 Hz presence sampler (0 absent frames), tap debounce, move-wins-over-hover, per-template ask; G7 screenshot queue per monitor and a sentinel test (main=green, overlay=magenta are present for a plain `screencapture` and absent from the queue PNGs); G2 pill drag through the real pointer path + position memory.


## M1 checks (live transcription, QA review)

* `tools/qa/m1-e2e.mjs` (macOS, qa build, **no audio output**: `LOWK_QA_SYSTEM_FILE` / `LOWK_QA_ME_FILE` replay files as the System / Me streams, qa builds only): stage `live` (real local model: overlay subtitles, finals in the call `.md`, Whisper correction, Ask on a partial, «Мой ответ» until the next Ask, stop -> home), `noengine` (empty `$HOME`: demo fallback is explained), `install` (real download card: progress, cancel, resume, corrupt file). Needs the worker (`cargo build --release --manifest-path workers/lowk-stt-worker/Cargo.toml`) and the models in `~/Library/Application Support/Lowk/models`. Results: `docs/qa/m1/`.
* `tools/qa/m1-e2e.mjs --stage loading,bundle,warmup` (also run by `just qa` when the model and worker exist; `bundle` wants a prior `just build`): cold-start loading state in overlay + main window (worker delayed with `LOWK_QA_LOAD_DELAY_MS`), buffered audio transcribed with early journal timestamps, `Lowk.app` ships a working worker, a bundle-shaped layout starts a REAL session with subtitles (no env, dev fallback off), and the background warm-up worker runs at nice 19.
* Audible (opt-in, real speakers, display must be awake, volume untouched): `tools/audio/isolation.sh`, and `LOWK_AUDIBLE_NO_MIC=1` (when the speakers are muted) `node tools/qa/m1-e2e.mjs --stage audible-app,audible-system`: per-app tap vs a second sound, real-capture latency, WER/CER, echo into «Я», our own app sound excluded from whole-system capture. Results: [M1-QA-REVIEW.md](M1-QA-REVIEW.md) «Тесты со звуком».
* `cargo run --release -p lowk-session --example stt_replay -- --models <dir> --probe-ask` replays the 20 owner recordings through the full session path and prints word-to-screen latency, an estimator audit (lag by position in the utterance, exact at the utterance end) and an `ask()` probe. Run it on a quiet machine: other processes inflate the tail.
* `ui/e2e/m1-engine.spec.ts`: the model download card (not installed / progress / cancel / checksum failure).


## Bundle QA (`just qa-bundle`): the product, not a dev build

Dev builds (`just dev`, the `qa` build) run under the terminal / Claude host app and **inherit its Microphone, Screen Recording, Input Monitoring and Accessibility grants**; they are also never signed. So permission, signing, Info.plist and "works in the real app" bugs were invisible to every check above (the M1 owner test found: crash on the mic prompt, permissions that never showed as granted, demo instead of transcription, dead hotkeys). `just qa-bundle` closes that gap. See [M1-FIX-REPORT.md](M1-FIX-REPORT.md).

* **Signing.** `just build` signs the bundle (`tools/sign-bundle.sh`). `LOWK_SIGN_IDENTITY="Lowk Dev"` (a self-signed Code Signing certificate the owner creates once: [setup/macos-signing.md](setup/macos-signing.md)) is passed to Tauri (`bundle.macOS.signingIdentity`) and used for the final signature. Unset: ad-hoc signature with `--identifier dev.lowk.app` and a designated requirement pinned to that identifier (the default, a cdhash, changes every build and orphans all TCC grants). Unsigned-as-a-bundle (`Info.plist=not bound`, random `lowk-<hash>` identifier) is what shipped before.
* **Checks** (`tools/qa/bundle-qa.mjs`, macOS): builds the bundle (`--no-build` to reuse it; `--no-run` for static checks only), then (1) `Info.plist` contains every privacy usage key we trigger (microphone, audio capture, screen capture), (2) `codesign` Identifier == `CFBundleIdentifier`, strict verification, designated requirement pinned to the id, sidecar worker signed, no QA channel string in the binary, (3) launches the bundle with a clean `HOME` and data dir, and reads `lowk.log` (the bundle writes it to `<data dir>/lowk.log`): alive after 9 s, no crash report, `demo_allowed=false`, no engine ready on a clean profile, startup permission states, bind report (plain combos must all be active with no extra permission).
* **Silent mode** (`--silent`): launches the bundle in the background (`open -g`) and synthesizes no key presses (no ⌘⇧R/⌘⇧S/⌘⇧A, reported as `SKIP`), so it can run while the owner uses the Mac. Nothing in the script plays sound. It also replays three owner recordings from files through the BUNDLED worker for the language sets ru+en and pl+en (`tools/qa/stt-lang-check.py --quick`; needs the engine models of this Mac, otherwise reported as NEEDS-GRANT).
* **Verdicts.** `PASS`, `FAIL` (exit 1) and `NEEDS-GRANT`: a permission the bundle does not have yet (the script never touches TCC and cannot grant it) is listed as an owner action and is never counted as a pass. Typical: Input Monitoring for the hold binds, Screen Recording, Accessibility.
* **Not machine-checkable:** that a grant survives a rebuild (needs the owner to grant once, rebuild with the same `LOWK_SIGN_IDENTITY`, and see the grants still on), the TCC prompts themselves, real hotkey presses. The orchestrator's smoke test of the bundle (computer use) covers those before the owner sees a build.
* GitHub Actions are not part of this: bundle QA runs locally on the owner's Mac.
