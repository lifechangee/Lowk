<p align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/design/logo/final/lowk-lockup-dark.svg"><img src="docs/design/logo/final/lowk-lockup-light.svg" alt="Lowk" width="300"></picture></p>

# Lowk

Lowk is an open-source (AGPL-3.0-or-later) real-time call assistant for macOS and Windows: live transcription of the microphone and of a chosen app's audio, a local Markdown vault of calls and documents, and on-demand answers from an LLM shown in a stealth overlay that is hidden from many screen-share paths. No telemetry; the only automatic network call is the update check (can be turned off).

Russian version: [README.ru.md](README.ru.md)

## Status

**Pre-alpha (milestone M0, skeleton).** Nothing is usable yet. Windows code is written and checked for compilation only; it has not been run on real hardware ("compiles, unverified").

Design documents: [docs/BRIEF.md](docs/BRIEF.md), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/ROADMAP.md](docs/ROADMAP.md).

## Build

Requirements: Rust (version pinned in `rust-toolchain.toml`), Node 22 + pnpm 10, [`just`](https://github.com/casey/just), the Tauri CLI (`cargo install tauri-cli --version "^2" --locked`), `cargo-deny`. macOS needs Xcode command line tools; Windows needs the MSVC build tools and WebView2.

```sh
just dev        # Tauri + Vite with hot reload
just build      # production build
just bindings   # regenerate ui/src/lib/ipc/bindings.ts from Rust types
just ci         # fmt, clippy, dependency rules, license check, tests, ui checks
```

Layout: `crates/` (13 Rust crates, `lowk-types` at the bottom), `ui/` (Svelte 5 + Vite), `tools/xtask`, `docs/`. See section 1 of the architecture document.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Contributions are accepted under DCO sign-off.
