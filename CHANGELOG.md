# Changelog

All notable changes to Lowk are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0]

First public release (macOS 14.4+ on Apple Silicon, Windows 11; Windows 10 with whole-system audio only).

### Added
- Live transcription of your microphone ("Me") and a call app or the whole system ("System"), local by default (Nemotron streaming with a Whisper pass that corrects finished phrases) or through Deepgram and Soniox with your own key. Per-call languages.
- Overlay with subtitles and answers that is excluded from screen sharing where the OS allows it, an edge pill when idle, an adaptive scrim that keeps text legible on any background, and a badge in the main window that reports whether the overlay is hidden from screen sharing, with a self-test.
- Ask on a hotkey with prompt templates, full-screen or area screenshots as context, and "My answer" to log what you said next to the AI hint. Cloud answers through OpenRouter (your key); local answers through Ollama or LM Studio.
- Summaries and tags after a call, including long lectures, and re-transcription of a saved recording with a stronger pass.
- A Markdown vault compatible with Obsidian: folders with instructions, calls as `.md` files, documents (PDF, DOCX, PPTX, XLSX, CSV, TXT, MD, images with OCR), pinned documents, hybrid keyword and semantic search, trash, folders that never use cloud models.
- Microphone modes (on, off, auto) per call and per folder.
- First-run setup: language, recording-law notice, audio source, permissions with live status, hardware check with model download, optional API keys, notes folder, hotkeys. A Permissions page in Settings shows the same status afterwards.
- Windows support: NSIS installer, whole-system and per-app audio capture (Windows 11), native stealth flags.
- Russian and English interface. No telemetry; the only automatic request is a daily update check that can be turned off.

### Known limitations
- Installers are not signed or notarized (see the README for the Gatekeeper and SmartScreen steps).
- Windows has not been verified on real hardware yet.
- No speaker diarization, vault encryption or sync.

[Unreleased]: https://github.com/lifechangee/Lowk/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/lifechangee/Lowk/releases/tag/v0.1.0
