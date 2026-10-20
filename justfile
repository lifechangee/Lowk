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
    pnpm --dir ui check
    cargo deny check

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
