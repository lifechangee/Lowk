# Lowk task runner. `just --list` for everything.
set shell := ["bash", "-cu"]

default:
    @just --list

# Install frontend dependencies (frozen lockfile).
setup:
    pnpm --dir ui install --frozen-lockfile

# Run the app with hot reload (Tauri + Vite).
dev: setup
    cd crates/lowk-app && cargo tauri dev

# Build the local speech worker and stage it as the Tauri sidecar (needs CMake; Metal on macOS).
worker:
    tools/build-worker.sh
    tools/build-embed-worker.sh

# Production build: the app bundle (macOS: target/release/bundle/macos/Lowk.app) with the
# STT worker and the embedder sidecar inside (externalBin, Contents/MacOS/lowk-stt-worker, lowk-embed-worker).
# Models are downloaded on demand.
# macOS: signed with LOWK_SIGN_IDENTITY (a "Lowk Dev" code-signing cert, docs/setup/macos-signing.md) so privacy
# grants survive rebuilds; without it the app is ad-hoc signed with Identifier = bundle id (tools/sign-bundle.sh).
build: setup worker
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/lowk-app
    cfg=(--config tauri.bundle.conf.json)
    # Default to the trusted "Lowk Dev" identity when it exists (stable TCC + keychain grants across rebuilds).
    if [ -z "${LOWK_SIGN_IDENTITY+x}" ] && [ "$(uname -s)" = Darwin ] \
       && security find-identity -v -p codesigning 2>/dev/null | grep -q '"Lowk Dev"'; then
      export LOWK_SIGN_IDENTITY="Lowk Dev"
    fi
    if [ -n "${LOWK_SIGN_IDENTITY:-}" ]; then
      cfg+=(--config "{\"bundle\":{\"macOS\":{\"signingIdentity\":\"$LOWK_SIGN_IDENTITY\"}}}")
    fi
    # S4: WebRTC AEC3 ships in macOS bundles (needs meson + ninja at build time, installed via uv).
    feat=()
    if [ "$(uname -s)" = Darwin ]; then
      export PATH="$HOME/.local/bin:$PATH"
      if ! command -v meson >/dev/null || ! command -v ninja >/dev/null; then uv tool install meson && uv tool install ninja; fi
      feat=(-- --features aec)
    fi
    cargo tauri build "${cfg[@]}" --bundles app ${feat[@]+"${feat[@]}"}
    cd ../..
    if [ "$(uname -s)" = Darwin ]; then tools/sign-bundle.sh; fi

# Regenerate the typed IPC bindings (ui/src/lib/ipc/bindings.ts).
bindings:
    cargo xtask bindings

fmt:
    cargo fmt --all

# fmt check, clippy, ui type check, dependency rules, license check.
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo xtask check-deps
    QA_GUARD_COMPILE=0 tools/qa/check-no-qa-release.sh
    pnpm --dir ui check
    cargo deny check

# UI visual + layout suite (Playwright, chromium + webkit, mock IPC). Baselines are local (gitignored).
test-ui-visual:
    pnpm --dir ui exec playwright test --update-snapshots=missing

# Re-record the local pixel baselines (do this on main BEFORE you change the UI).
test-ui-visual-update:
    pnpm --dir ui exec playwright test --update-snapshots

# Real-app QA (macOS, qa build): control channel, ScreenCaptureKit captures, frame sampler.
qa-app:
    node tools/qa/app-qa.mjs

# Gallery: docs/qa/report.html from the latest runs.
qa-report:
    node tools/qa/report.mjs

# Builds the signed bundle (`just build`) and checks THE BUNDLE, not a dev build: Info.plist privacy keys,
# codesign Identifier == bundle id, clean-profile launch, no demo, bind/permission status. Permissions the
# bundle lacks are reported as NEEDS-GRANT (needs a local grant), never as a pass. `--no-build`, `--no-run` supported.
qa-bundle *args:
    node tools/qa/bundle-qa.mjs {{args}}

# Manual live check: needs a local macOS GUI session and grants. The qa-feature bundle (debug profile, qa control channel) at
# target/debug/bundle/macos/Lowk.app, signed with the SAME identity ("Lowk Dev" by default) and bundle id as `just build`,
# so the existing Screen Recording / Input grants apply and no new prompt appears. Never hand this build to anyone.
build-qa: setup worker
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/lowk-app
    cfg=(--config tauri.bundle.conf.json)
    if [ -z "${LOWK_SIGN_IDENTITY+x}" ] && [ "$(uname -s)" = Darwin ] \
       && security find-identity -v -p codesigning 2>/dev/null | grep -q '"Lowk Dev"'; then
      export LOWK_SIGN_IDENTITY="Lowk Dev"
    fi
    if [ -n "${LOWK_SIGN_IDENTITY:-}" ]; then
      cfg+=(--config "{\"bundle\":{\"macOS\":{\"signingIdentity\":\"$LOWK_SIGN_IDENTITY\"}}}")
    fi
    feat=(--features aec,qa)
    export PATH="$HOME/.local/bin:$PATH"
    if ! command -v meson >/dev/null || ! command -v ninja >/dev/null; then uv tool install meson && uv tool install ninja; fi
    cargo tauri build --debug "${cfg[@]}" --bundles app -- "${feat[@]}"
    cd ../..
    tools/sign-bundle.sh target/debug/bundle/macos/Lowk.app

# Manual (local macOS GUI session, Screen Recording granted): tone e2e inside the qa bundle (just build-qa first): Lowk draws known backdrops in its own window, the overlay
# is captured over them at 5 positions -> docs/qa/tone-e2e/. See docs/testing.md.
qa-tone *args:
    node tools/qa/tone-e2e.mjs {{args}}

# CI guard: the qa feature can never reach a release build.
qa-guard:
    tools/qa/check-no-qa-release.sh

# Everything available on this machine + the gallery report. See docs/testing.md.
qa:
    tools/qa/run-all.sh

# One-time setup: rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-msvc
# Cross-target clippy so cfg-gated dead code on Linux/Windows is caught locally (CI lints on ubuntu).
# Windows needs a dummy llvm-rc on PATH (created here); lowk-app is excluded (WebKitGTK on Linux).
# Crates with C sources (rusqlite's bundled SQLite in lowk-index) cannot be compiled for a foreign target
# from this host; clippy never links, so a no-op compiler (`true`) and an archiver that only creates the
# output file (fake-ar: `ar cq OUT ...` or `lib /OUT:OUT ...`) satisfy their build scripts.
ci-cross:
    mkdir -p target/fakebin && printf '#!/bin/sh\nexit 0\n' > target/fakebin/llvm-rc && chmod +x target/fakebin/llvm-rc
    printf '#!/bin/sh\nfor a in "$@"; do case "$a" in -out:*|-OUT:*|/out:*|/OUT:*) : > "${a#*:}"; exit 0;; esac; done\n: > "$2"\n' > target/fakebin/fake-ar && chmod +x target/fakebin/fake-ar
    CC_x86_64_unknown_linux_gnu=true AR_x86_64_unknown_linux_gnu="$PWD/target/fakebin/fake-ar" cargo clippy --workspace --exclude lowk-app --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
    CC_x86_64_pc_windows_msvc=true AR_x86_64_pc_windows_msvc="$PWD/target/fakebin/fake-ar" PATH="$PWD/target/fakebin:$PATH" cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings

# Everything CI runs, incl. Linux and Windows cross-target clippy.
ci: lint ci-cross
    cargo test --workspace
    pnpm --dir ui test
    pnpm --dir ui build
    pnpm --dir ui size

# Long-session soak (NOT part of `just ci`; silent, headless, nothing is played): loops the Russian bench clips as the
# System stream through the real ring -> session -> local STT worker (+ Whisper correction) -> vault .md -> search index
# and records RSS, word-to-screen latency over time, drops, worker restarts, transcript completeness, .md size, index
# update time -> target/soak-report.md. Needs the engine models (default: the app's data dir) and a built lowk-stt-worker
# (`just worker`, or LOWK_STT_WORKER=...). `just soak 0.25 8` = 15 min of audio at 8x (smoke); default 2 h in real time
# (takes 2 h; run it in the background). Details: docs/testing.md.
soak hours="2" speed="1" *args:
    #!/usr/bin/env bash
    set -euo pipefail
    models="${LOWK_MODELS:-$HOME/Library/Application Support/Lowk/models}"
    triple="$(rustc -vV | sed -n 's/^host: //p')"
    if [ -z "${LOWK_STT_WORKER:-}" ]; then
      for w in "crates/lowk-app/binaries/lowk-stt-worker-$triple" workers/lowk-stt-worker/target/release/lowk-stt-worker; do
        [ -x "$w" ] && export LOWK_STT_WORKER="$PWD/$w" && break
      done
    fi
    [ -n "${LOWK_STT_WORKER:-}" ] || { echo "no lowk-stt-worker: run 'just worker' or set LOWK_STT_WORKER"; exit 2; }
    [ -d "$models/nemotron-local" ] || { echo "engine model missing in $models (install it from the app's Settings; not downloaded here)"; exit 2; }
    cargo run --release -p lowk-app -- --selfcheck-soak --audio "${LOWK_BENCH_AUDIO:-fixtures/stt}" --models "$models" \
      --hours {{hours}} --speed {{speed}} --report "${LOWK_SOAK_REPORT:-target/soak-report.md}" --csv "${LOWK_SOAK_CSV:-target/soak-series.csv}" {{args}}

# Compile-check the Windows target from any host.
check-windows:
    cargo check --workspace --target x86_64-pc-windows-msvc

# Stealth probe against the real app (macOS, debug build, Screen Recording needed).
stealth-probe-app:
    tools/stealth-probe/run-app.sh
