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

# Production build: the app bundle (macOS: target/release/bundle/macos/Lowk.app) with the
# STT worker inside (externalBin, Contents/MacOS/lowk-stt-worker). Models are downloaded on demand.
build: setup worker
    cd crates/lowk-app && cargo tauri build --config tauri.bundle.conf.json --bundles app

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

# CI guard: the qa feature can never reach a release build.
qa-guard:
    tools/qa/check-no-qa-release.sh

# Everything available on this machine + the gallery report. See docs/QA.md.
qa:
    tools/qa/run-all.sh

# One-time setup: rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-msvc
# Cross-target clippy so cfg-gated dead code on Linux/Windows is caught locally (CI lints on ubuntu).
# Windows needs a dummy llvm-rc on PATH (created here); lowk-app is excluded (WebKitGTK on Linux).
ci-cross:
    cargo clippy --workspace --exclude lowk-app --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
    mkdir -p target/fakebin && printf '#!/bin/sh\nexit 0\n' > target/fakebin/llvm-rc && chmod +x target/fakebin/llvm-rc
    PATH="$PWD/target/fakebin:$PATH" cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings

# Everything CI runs, incl. Linux and Windows cross-target clippy.
ci: lint ci-cross
    cargo test --workspace
    pnpm --dir ui test
    pnpm --dir ui build
    pnpm --dir ui size

# Compile-check the Windows target from any host (see docs/DECISIONS-M0.md).
check-windows:
    cargo check --workspace --target x86_64-pc-windows-msvc

# Stealth probe against the real app (macOS, debug build, Screen Recording needed).
stealth-probe-app:
    tools/stealth-probe/run-app.sh
