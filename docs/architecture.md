# Lowk architecture

A short overview of how Lowk is put together. Vocabulary: **Me** = microphone stream, **System** = captured app/system audio stream, **Call** = one recording stored as one `.md` file, **Vault** = the user's folder of Markdown.

## Shell and frontend

- Tauri 2 (Rust core + system WebView) with two permanent windows: `overlay` (stealth, created at startup, never destroyed) and `main` (normal window).
- Frontend: Svelte 5 + Vite + TypeScript, plain CSS with design tokens, RU/EN i18n. Two HTML entries (overlay, main) share `ui/src/lib`. IPC types are generated from Rust (`just bindings` writes `ui/src/lib/ipc/bindings.ts`).
- Each window has its own capability file; the frontend has no permission to change window or stealth flags.

## Crates

`crates/lowk-types` is the contract at the bottom (ids, events, settings schema, error codes, traits). Feature crates depend on it, never on each other; wiring happens in `lowk-session` and `lowk-app`. `cargo xtask check-deps` enforces this (part of `just lint`), and `cargo-deny` enforces licenses.

| Crate | Responsibility |
|---|---|
| `lowk-types` | Types that cross a crate or IPC boundary, settings schema, trait definitions |
| `lowk-platform` | Hardware probe, permissions, OS keychain, app directories, locale |
| `lowk-audio` | Per-OS capture (Core Audio process taps on macOS, WASAPI loopback on Windows), rings, resampling, echo cancellation |
| `lowk-stt` | Speech engine trait, VAD, post-processing, sidecar host, model manifests |
| `lowk-store` | Vault, `.md` codecs, live-call journal, `settings.toml`, prompt templates |
| `lowk-index` | SQLite index (FTS5 over stemmed RU+EN text, vectors), hybrid search, retrieval |
| `lowk-ingest` | PDF, DOCX, PPTX, XLSX, CSV, TXT, MD and images (OCR) to Markdown |
| `lowk-capture` | Screenshots (display or area) and region grabs |
| `lowk-llm` | OpenAI-compatible streaming client, provider profiles, append-only chat ledger, prompts, summaries |
| `lowk-stealth` | Window policy, capture exclusion, overlay mode machine, adaptive tone, panic hide |
| `lowk-input` | Bind model and hotkey, hold-key and mouse-button backends |
| `lowk-session` | Session state machine, event bus, transcript buffer, Ask flow, Q&A links |
| `lowk-app` | The Tauri shell: windows, capabilities, command and event glue, updater |

Out-of-process engines live in `workers/`: `lowk-stt-worker` (local speech recognition) and `lowk-embed-worker` (local embeddings). They speak a small stdio protocol (JSON control lines, binary PCM frames), so a crash or hang in native inference cannot take the app down. Models are never part of the installer; they are downloaded on demand and checked against pinned SHA-256 sums.

## Data flow

```
 audio threads (Core Audio / WASAPI)  ->  ring buffer  ->  per-stream worker: resample to 16 kHz, echo cancel on Me
   -> per-stream STT (local sidecar or cloud socket)  ->  Partial / Final events
   -> lowk-session event bus  ->  transcript buffer, call writer (journal + atomic .md), UI emitter (overlay + main)
 hotkey / mouse (lowk-input thread)  ->  bind event  ->  session / overlay
 Ask: transcript since the last answer + folder instructions + retrieved chunks (+ screenshot) -> lowk-llm -> streamed answer
```

Rules: real-time callbacks only copy into rings (no allocation, no locks); all window-affecting native calls run on the main thread; the bus carries low-rate events only; a lagging subscriber resyncs from a session snapshot.

## Storage

- The vault of `.md` files is the source of truth (Obsidian-compatible: frontmatter, speaker lines, callouts). `.lowk/index.db` is a rebuildable cache.
- Live calls are journaled to JSONL and materialized to `.md` atomically, so a crash loses at most a few seconds.
- Settings are `settings.toml` in the app data directory (macOS `~/Library/Application Support/Lowk`, Windows `%APPDATA%\Lowk`), with defaults for every key and schema migration.

## Platform backends

| | macOS 14.4+ (Apple Silicon) | Windows 10 2004+ / 11 |
|---|---|---|
| System audio | Core Audio process tap | WASAPI process loopback |
| Capture exclusion | `NSWindow.sharingType = .none` | `WDA_EXCLUDEFROMCAPTURE` |
| Overlay window | non-activating `NSPanel`, screen-saver level, all spaces | topmost tool window, no-activate |
| Plain hotkeys | global shortcut (no permission) | global shortcut |
| Hold keys, mouse buttons | `CGEventTap` (Input Monitoring) | low-level keyboard and mouse hooks |
| OCR | Apple Vision | Windows OCR |

## Security and privacy model

- No telemetry and no account. The only automatic network request is a daily update check (switchable off).
- API keys live in the OS keychain only: never in settings files, the vault, logs or the WebView, and never returned to JavaScript.
- Cloud features (answers, summaries, cloud transcription, cloud embeddings) send data only when used. Folders marked local-only never reach a cloud model.
- Stealth is fail-closed: apply the exclusion flags, read them back, and only then show the overlay; on a mismatch the overlay hides. It reduces visibility in many screen-share paths but is not a guarantee; see the user guide.
- Demo and fake data exist only behind the `qa` cargo feature, which cannot compile in release builds (a `compile_error!` plus a CI guard).

## Testing

See [testing.md](testing.md). Licensing: the project is AGPL-3.0-or-later; dependency licenses are checked by `cargo-deny` and a frontend license check in CI.
