# Testing

Unit tests alone do not catch visual and overlay bugs, so Lowk has several layers of checks. Every UI or overlay change should be verified with them.

## What "green" means

1. Local `just ci` passes, including `just ci-cross` (clippy `-D warnings` for `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`; one-time setup: `rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-msvc`, the recipe creates the dummy `llvm-rc` itself).
2. The GitHub Actions run for your pushed commit is green (lint, test-macos, test-windows). Check with `gh run watch`.

macOS-only `just ci` is not enough: Linux/Windows cfg-gated dead code and CRLF issues only show up in the cross-target lint.

## Layers

| Layer | Command | What it checks |
|---|---|---|
| Rust and UI unit tests | `just ci` | `cargo test --workspace`, vitest, lint, dependency rules, licenses, UI type check and build, bundle size. |
| UI visual + layout suite | `just test-ui-visual` | Playwright, **chromium and webkit** (the app runs in WKWebView), against the mock IPC (`vite` on port 1431, never 1420). Every main-window route and state, every settings page, all onboarding steps, search palette, overlay states, 3 backgrounds x opacity 0/60/90, RU and EN, 2 sizes. Per state: pixel baseline + layout lint. |
| Regression tests | `ui/e2e/regressions.spec.ts` (part of the above) | One test per previously reported UI bug that the browser can observe. |
| Real-app QA | `just qa-app` (macOS) | A `qa` build of the real app driven over a localhost token channel. Lints the real WKWebView DOM, captures the real windows (including the overlay) through ScreenCaptureKit, samples the overlay at >=100 Hz across mode switches, checks streamed answers for duplicated words. |
| Gallery | `just qa-report` -> `docs/qa/report.html` | Every screenshot (chromium, webkit, real app) next to the matching mockup frame, failures red, known issues amber. |
| Everything available | `just qa` | guard + UI suite + real app + report. |
| Bundle QA | `just qa-bundle` (macOS) | Checks the bundled, signed `Lowk.app`, not a dev build (see below). |

Pixel baselines are machine specific (font rendering), so they are gitignored. Record them before you start: `just test-ui-visual-update`, then `just qa`. After your change run `just qa` again and look at the gallery; if the look changed on purpose, run `just test-ui-visual-update` and look at the new screenshots.

## Lint rules (`ui/e2e/lib/lint.ts`, `lib/contrast.ts`)

`overflow` (text clipped without ellipsis, or outside a non-scrolling clipping ancestor), `outside` (outside the viewport), `text-overlap` (text boxes of unrelated elements intersect; popovers exempt), `box-overlap` (in-flow siblings intersect), `flex-gap` (a chip touches neighbouring text or chip on a line), `grid` (key containers: position/height/padding/gap not multiples of 4px), `contrast` (pixel based: text is made transparent, the page screenshotted, text colour compared against the 10th-percentile worst background pixels; below 3:1 fails, 3..4.5 is a warning), `dup-words` (consecutive duplicated words).

The same `lintInPage` runs inside the real webview in `just qa-app`. Never loosen a rule to make a check pass. New UI states must be registered in `ui/e2e/lib/states.ts`. A product bug the lint finds and you cannot fix now goes into `ui/e2e/known.ts` with a comment.

## Real-app QA mode (`qa` cargo feature)

* `cargo build -p lowk-app --features qa,custom-protocol` (debug only). **Never in release**: `crates/lowk-app/src/lib.rs` has a `compile_error!` for `qa` without `debug_assertions`, and `tools/qa/check-no-qa-release.sh` (run by `just lint`/CI, `just qa-guard`) verifies that no default feature, recipe or workflow enables it and that a release build with the feature fails to compile.
* The app binds `127.0.0.1:$LOWK_QA_PORT` (random if unset), requires header `X-Lowk-Qa-Token`, rejects foreign `Host` headers, and writes `{port, token, pid}` to `$LOWK_DATA_DIR/qa.json`. `LOWK_QA_MULTI=1` disables the single-instance hand-off so a QA instance can run beside a developer's own `just dev`; it uses its own temp data dir and vault.
* Commands (POST `/cmd`, body = one line): `bind <action> <down|up>`, `hover-state`, `pill-state`, `screenshot`, `nav <hash>`, `eval <main|overlay|pill> <js expr>`, `win <label>`, `pid`, `opacity N`, `font PX`, `lang ru|en`, `mode passive|interactive|moving`, plus the stealth-probe set (`overlay-show`, `demo-start`, `demo-stop`, `overlay-move X Y`, `exclusion on|off`, ...).
* **Capturing the overlay**: the overlay is excluded from capture (`NSWindowSharingNone`), and ScreenCaptureKit does not deliver such a window while the exclusion is on, so the QA run sends `exclusion off` before capturing and restores it afterwards. Only qa builds can do that. The presence sampler needs no lift: it polls window-server metadata (`CGWindowList` on-screen flag and alpha) at ~120-250 Hz.
* Needs Screen Recording permission for the terminal that runs `node tools/qa/app-qa.mjs`.

## Session end-to-end (`tools/qa/session-e2e.mjs`)

macOS, qa build, **no audio output**: `LOWK_QA_SYSTEM_FILE` / `LOWK_QA_ME_FILE` replay files as the System / Me streams (qa builds only).

* Stage `live` (real local model): overlay subtitles, finals in the call `.md`, Whisper correction, Ask on a partial, the «Мой ответ» capture, stop -> home. Stage `noengine` (empty `$HOME`: the demo fallback is explained). Stage `install` (download card: progress, cancel, resume, corrupt file). Needs the worker (`cargo build --release --manifest-path workers/lowk-stt-worker/Cargo.toml`) and the models in `~/Library/Application Support/Lowk/models`.
* `--stage loading,bundle,warmup`: cold-start loading state, buffered audio transcribed with early journal timestamps, the bundle ships a working worker, a bundle-shaped layout starts a real session, the background warm-up worker runs at low priority.
* Audible stages (opt-in, real speakers, display awake, volume untouched): `tools/audio/isolation.sh` and `--stage audible-app,audible-system`.
* `cargo run --release -p lowk-session --example stt_replay -- --models <dir> --probe-ask` replays recordings through the full session path and prints word-to-screen latency and an `ask()` probe. Run it on a quiet machine.
* Reference texts for the recordings are tracked in `fixtures/stt/*.txt`; the `.m4a` recordings are not in the repository. Put your own in that directory or point `LOWK_BENCH_AUDIO` at them.
* `ui/e2e/engine-install.spec.ts`: the model download card.

## Tone end-to-end (`just qa-tone`)

Needs `just build-qa` first. Lowk draws known backdrops in its own window and the overlay is captured over them at 5 positions; results go to `docs/qa/tone-e2e/`. It needs a local macOS GUI session with Screen Recording already granted.

## Soak (`just soak`)

Not part of `just ci`. Silent and headless: loops the Russian clips from `fixtures/stt` as the System stream through ring, session, local STT worker (with the Whisper correction pass), vault `.md` and search index, and records RSS, word-to-screen latency over time, drops, worker restarts, transcript completeness, `.md` size and index update time. `just soak 0.25 8` is a 15-minute smoke run at 8x; the default is 2 h in real time. Needs the engine models and a built `lowk-stt-worker`.

## Bundle QA (`just qa-bundle`)

Dev builds (`just dev`, the `qa` build) run under the terminal and **inherit its Microphone, Screen Recording, Input Monitoring and Accessibility grants**, and they are never signed. Permission, signing, Info.plist and "works in the real app" bugs are invisible to them, so `just qa-bundle` checks the bundle itself.

* **Signing.** `just build` signs the bundle (`tools/sign-bundle.sh`). `LOWK_SIGN_IDENTITY="Lowk Dev"` (a self-signed Code Signing certificate: [setup/macos-signing.md](setup/macos-signing.md)) is passed to Tauri and used for the final signature. Unset: ad-hoc signature with `--identifier dev.lowk.app` and a designated requirement pinned to that identifier (the default, a cdhash, changes every build and orphans all TCC grants).
* **Checks** (`tools/qa/bundle-qa.mjs`, macOS): builds the bundle (`--no-build` to reuse it; `--no-run` for static checks only), then (1) `Info.plist` contains every privacy usage key we trigger, (2) `codesign` Identifier == `CFBundleIdentifier`, strict verification, designated requirement pinned to the id, sidecar worker signed, no QA channel string in the binary, (3) launches the bundle with a clean `HOME` and data dir and reads `lowk.log`: alive after 9 s, no crash report, `demo_allowed=false`, no engine ready on a clean profile, startup permission states, bind report.
* **Documents self-check.** The bundle's own binary has a headless diagnostic mode, `lowk --selfcheck-m2 --fixtures fixtures/ingest [--model GGUF] [--ocr] [--require-worker] [--live]` (no window, no sound; code in `crates/lowk-app/src/m2/selfcheck.rs`, also run offline by `cargo test -p lowk-app`). It covers ingest of each document format, progress events, search, incremental indexing, rebuilding `index.db`, hybrid search with the bundled embedding worker, the Ask flow against a stub OpenAI server, local-only folders (cloud LLM refused with zero requests sent), summary, and folder export/import. `--live` makes one tiny real OpenRouter request (key from the environment or `~/.lowk-test-keys`, never printed).
* **Silent mode** (`--silent`): launches the bundle in the background (`open -g`) and synthesizes no key presses, so it can run while the Mac is in use. Nothing plays sound.
* **Verdicts.** `PASS`, `FAIL` (exit 1) and `NEEDS-GRANT`: a permission the bundle does not have yet (the script never touches TCC) is listed as needing a grant and is never counted as a pass. Typical: Input Monitoring for the hold binds, Screen Recording, Accessibility.
* **Not machine-checkable:** that a grant survives a rebuild, the TCC prompts themselves, and real hotkey presses. Check those by hand on the bundle before a build is handed out.
* GitHub Actions do not run bundle QA; it runs locally on a Mac.
