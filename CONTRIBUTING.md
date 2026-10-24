# Contributing to Lowk

Thanks for helping. Lowk is pre-alpha; the codebase moves fast, so open an issue before a large change.

Read first: [docs/BRIEF.md](docs/BRIEF.md) (product decisions, Russian), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/QA.md](docs/QA.md). Code, comments and docs are in English; README and the user guide also have Russian versions, keep both in sync.

## Dev setup

Requirements: Rust (pinned in `rust-toolchain.toml`), Node 22, pnpm 10, [`just`](https://github.com/casey/just), `cargo install tauri-cli --version "^2" --locked`, `cargo-deny`. macOS: Xcode command line tools, CMake. Windows: MSVC build tools, WebView2. For cross-target lint run once: `rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-msvc`.

```sh
just setup      # frontend dependencies
just dev        # app with hot reload
just lint       # fmt check, clippy -D warnings, dependency rules, ui check, cargo-deny
just ci         # lint + cross-target lint + tests + UI checks (includes just ci-cross)
just bindings   # after changing Rust types used over IPC
```

Never commit generated or machine-specific files: Playwright pixel baselines are local and gitignored, and temp files, models and virtualenvs you create must be removed when done.

## What "green" means

A change is green only when both hold:

1. Local `just ci` passes, including `just ci-cross` (Linux and Windows clippy catch cfg-gated dead code and CRLF issues).
2. The GitHub Actions run for your pushed commit is green (lint, test-macos, test-windows). Check with `gh run watch`.

## UI and overlay changes

Unit tests do not catch visual and overlay bugs. Follow the rules in [docs/QA.md](docs/QA.md): record baselines before you start (`just test-ui-visual-update`), run `just qa` before and after, look at the gallery (`docs/qa/report.html`) next to the mockups, and attach it or the changed screenshots to the PR. New UI states must be registered in `ui/e2e/lib/states.ts`. Never loosen a lint rule to make something pass.

## Release gate

Builds that other people (including the product owner) use must be bundled, signed `Lowk.app` from `just build`, never a dev-mode build.

- Before a build is handed out, `just qa-bundle` must be green on the bundled app from a clean profile, plus a manual smoke test of the bundle covering every previously working feature.
- No silent fallbacks to demo or fake data in user-facing builds. Demo data lives only behind the `qa` cargo feature, which must never be enabled in release (CI enforces this).
- Every reported bug gets a permanent regression check that runs against the bundle (for UI bugs, a test in `ui/e2e` or `tools/qa/app-qa.mjs`).

## Architecture rules

`crates/lowk-types` and `deny.toml`, `Cargo.toml`, `.github/` are architect-gated: propose additive changes in an issue or PR description instead of editing them casually. Crate dependency direction is enforced by `cargo xtask check-deps` (part of `just lint`). Keep claims in docs accurate: if it is not implemented, say "planned".

## Pull requests

- One topic per PR, on a branch, not on `main`. Describe what changed and how you verified it.
- Commit messages: short imperative subject, optionally with a body. Use a prefix such as `feat(ui):`, `fix(index):`, `docs:`, `test:`.
- **No AI attribution** in commits or PR text: no `Co-Authored-By` lines for tools and no "generated with" footers.

## DCO sign-off

Contributions are accepted under the [Developer Certificate of Origin](https://developercertificate.org/). Sign every commit:

```sh
git commit -s -m "fix(store): handle empty folder"
```

This adds a `Signed-off-by: Your Name <you@example.com>` line, certifying that you wrote the change or have the right to submit it under the project's license, AGPL-3.0-or-later. For a missed sign-off, run `git commit --amend -s` (or `git rebase --signoff` for several commits).

## Reporting bugs and ideas

Use the issue templates. Security problems go through [SECURITY.md](SECURITY.md), not public issues.
