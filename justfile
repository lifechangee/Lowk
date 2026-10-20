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

# Production build of the app (no installer bundle in M0).
build: setup
    cd crates/lowk-app && cargo tauri build --no-bundle

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

# Everything CI runs on Linux/macOS.
ci: lint
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
