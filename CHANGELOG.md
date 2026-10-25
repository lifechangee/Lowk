# Changelog

All notable changes to Lowk are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Delete folder (sidebar ⋯ menu and folder page): the folder with all its calls and documents goes to the Trash after a confirmation with counts, and Restore brings everything back. Refused while a recording is running in it; Unsorted and Trash cannot be deleted.
- Move to trash in the ⋯ menu of call rows (folder list and Home), with Undo.
- Proxy support: every outbound connection (OpenRouter, cloud embeddings, Deepgram and Soniox streaming, model downloads, updates, key checks) now follows the system proxy (Windows Internet Settings, macOS network proxies) or the `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` / `NO_PROXY` variables, over HTTP CONNECT or SOCKS5. Settings → Network shows the detected proxy, lets you set one manually and checks the connection. PAC scripts are not supported. Localhost always connects directly.

### Changed
- Summaries fit the call: the model recognises a meeting, interview, lecture, broadcast or casual talk and writes the sections that suit it (for a lecture: topics, key ideas, terms and definitions, what was asked, what to learn) instead of always four fixed ones. Empty sections are dropped; tasks and questions to you keep their format; the folder's instructions override the automatic choice. Long recordings are condensed the same way.
- Log lines carry local time with the UTC offset (`2026-10-25T23:25:15.898+03:00`) instead of UTC.
- Interactive mode (typing in the overlay) now stays on until you toggle it or press Esc; losing focus to another app no longer switches it off.

### Fixed
- Model download: the «download the model» prompt of a call that could not start no longer appears while that model is already downloading (it shows the running download instead), and a second click can no longer start a second download of the same files. The progress shows one total for the whole set that never goes down. A stalled or dropped connection is detected and resumed automatically; a file that fails its checksum is downloaded again by itself, and only repeated failures show an error with a working Retry.
- Hold binds (click-hold, move-hold) could stay dead after the interactive mode was toggled, when a key release was lost. Lost key and mouse-button releases are now detected and repaired, and the Windows keyboard and mouse hooks are re-registered periodically and after a mode switch.

### Removed
- Tags: chips, the add-tag control, tag filter in search, AI tag suggestions and the related commands. Existing `tags:` in Markdown front matter are still read and never rewritten or stripped.

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
