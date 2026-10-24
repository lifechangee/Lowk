# Lowk user guide

[Русская версия](ru.md) · [Back to README](../../README.md)

Lowk is **pre-alpha**. This guide describes what is built on macOS. Windows release builds are experimental ([docs/release.md](../release.md)) and not verified on real hardware. Anything marked "planned" does not exist yet.

## Contents

1. [Install and the unsigned-app warning](#install)
2. [Permissions and why each is needed](#permissions)
3. [Your first call](#first-call)
4. [Hotkeys, including hold keys](#hotkeys)
5. [Languages](#languages)
6. [Folders, documents, search, summary](#vault)
7. [Local-only folders](#local-only)
8. [Stealth limits](#stealth-limits)
9. [Recording consent: your responsibility](#consent)

## Install

Test builds (macOS `.dmg`/`.app.zip`, and experimental Windows installers) are produced by GitHub Actions: see [docs/release.md](../release.md), which also describes the Windows SmartScreen warning. Otherwise build the app yourself (`just build`, see the [README](../../README.md)); it produces `target/release/bundle/macos/Lowk.app`. Copy it to Applications. Requirements: Apple Silicon, macOS 14 or newer.

Lowk is not notarized by Apple (it is signed ad hoc or with a self-signed certificate). The first launch will be blocked by Gatekeeper:

1. Double-click `Lowk.app`. macOS says it cannot verify the app. Close the dialog.
2. Open System Settings, Privacy & Security, scroll to the message about Lowk and click **Open Anyway**, then confirm. (Alternatively right-click the app, choose Open, then Open.)

A rebuilt app can look "new" to macOS, which may reset the permissions below. Re-grant them if transcription or hotkeys stop working after an update.

Keys and data: your OpenRouter or cloud-transcription key is stored in the macOS Keychain. Your vault is plain Markdown in `~/Documents/Lowk` unless you choose another folder (an Obsidian vault works).

## Permissions

The onboarding asks for everything up front, with a skip button. You can grant or change each later in Settings, Permissions, and in System Settings, Privacy & Security.

| Permission | Why | If you refuse |
|---|---|---|
| Microphone | Transcribe your own voice (labelled "Me") | No "Me" lines |
| System audio | Transcribe the call app you choose (labelled "System") | No other-side transcript |
| Screen recording | Screenshots you request, and the overlay adapting its colors to what is behind it. Lowk's own windows are excluded from the capture | No screenshots, no adaptive overlay |
| Input monitoring | Hold keys (hold ⌥ to move, hold ⌃ for mouse) and mouse side buttons as hotkeys. Lowk inspects only the keys you bound and does not log typing | Hold keys do not work; ⌥ move falls back to a press-to-start, press-to-stop toggle |
| Accessibility (optional) | Only to swallow a bound mouse button so the browser does not also get "Back" | Bound mouse buttons are still seen but also reach other apps |

If macOS keeps asking again, remove Lowk from the list in System Settings and add it back, then restart Lowk.

## First call

1. Launch Lowk and finish the onboarding. Pick the default audio source (a specific call app is best, so a YouTube tab does not leak in) and your call languages.
2. Local speech models are downloaded on demand. Install them from the prompt Lowk shows; there is a progress bar, cancel and resume. Without models Lowk cannot transcribe locally and tells you so; it does not fake output.
3. For AI answers, open Settings, Providers and paste your OpenRouter key (or connect Ollama or LM Studio). Without a provider the answer hotkey shows a notice instead of an answer.
4. Click **New call** (or press ⌘⇧R), choose a folder and the audio source, and start. The overlay appears on your main display with live subtitles: "Me" and "System".
5. When a question comes up, press ⌘↵. The text so far (including the unfinished sentence) is sent at once and the answer streams into the overlay. Anything you say afterwards appears as "My answer" until your next press. ⌘⇧↵ asks the model to add to what you already said.
6. Stop the recording. The call is saved as a Markdown file in its folder. Use **Summary** on the call page for key points, decisions and tasks.

By default only text is stored. Saving audio is a per-call toggle and is off by default.

## Hotkeys

Defaults on macOS (Windows: Ctrl instead of ⌘, Alt instead of ⌥). All are changeable in Settings, Hotkeys, including mouse side buttons.

| Action | Default |
|---|---|
| Ask AI (template "Brief") | ⌘↵ |
| Ask AI (template "Add to my answer") | ⌘⇧↵ |
| Screenshot (all displays) | ⌘⇧S |
| Screenshot of a region | ⌘⇧A |
| Interactive mode (typing in the overlay) | ⌘⇧I |
| Show or hide the overlay | ⌘\ |
| Start or stop recording | ⌘⇧R |
| **Hold ⌥**: move the overlay | hold |
| **Hold ⌃**: mouse becomes active over the overlay | hold |

Hold keys:

- **Hold ⌥.** The overlay snaps to the cursor and follows it while you hold, with no click and no cursor change; release to drop it. Works in any mode. Needs Input Monitoring; without it use the toggle fallback.
- **Hold ⌃.** While held, the mouse is active over the overlay (click buttons, pick a template, drag the header). Release and clicks pass through to the app below again, so the overlay never steals focus from your call.

You can bind other templates, including your own, to keys.

## Languages

The interface follows the system language (Russian or English) and can be switched in Settings. Separately, you choose the **call languages** (default Russian plus English, or your system language) in Settings and before each call. Recognition and the filter for stray characters are limited to the selected languages, and mixed speech ("what does EBITDA mean?") is supported. Only languages the engine supports are listed.

## Vault

The vault is a folder of Markdown files, openable in Obsidian as is: **Folder** (a project or client) contains **calls** (`.md`) and **documents**. System folders: Unsorted (calls without a folder) and Trash (deleted items are restored from here and kept until you empty it).

Main window: one sidebar with New call, Home, Search, the folder tree, Unsorted, Trash and Settings. A folder has tabs **Calls / Documents / Instructions**; a call has **Transcript / Summary / Questions to AI / Screenshots / Sources**. A short tour runs on first launch and can be repeated from Settings.

![Documents tab](../images/en-folder-docs.png)

Documents. Supported: PDF (text and scanned), DOCX, PPTX, XLSX, CSV, TXT, MD and images (OCR); Keynote is handled on macOS only (the bundle QA covered PDF, XLSX, DOCX, PPTX, CSV, MD and a scanned PDF). Drag files into the folder or use Add document. The original is kept next to a converted `.md`. If a file cannot be converted you see an error on it, never an empty document.

- **Pin** a document with the star to always include it in AI requests (up to a token budget shown in the UI).
- **Instructions** is a short text (about 500 tokens) always sent with requests for that folder.
- **Search** (⌘K, and a full results page): one line over calls, documents and screenshot text; filters by folder, tag and date; a hit opens the call at the minute or the document at the page. Semantic search uses a local embedding model (about 310 MB, downloaded on demand). While it is missing or building, results say "keyword" mode.
- **Summary** (button after a call): 3 to 5 key points, decisions, tasks and questions to you. It runs on a cloud model (DeepSeek via OpenRouter) with your key. The AI then suggests tags; you confirm them.
- **Export and import** the whole vault or one folder as a zip. Import never overwrites.

![Search](../images/en-search.png)

**Ask uses your context.** When you press the answer hotkey, Lowk adds the folder's instructions, pinned documents, relevant chunks of its documents and relevant parts of earlier calls in the same folder (not the current call, which is sent as the live delta). The sources appear as chips under the answer in the overlay and on the call's Sources tab; click one to open it. Screenshots in this guide use sample data from the UI test suite.

Caveats: this is pre-alpha. At the time of the M2 merge the CI run of the final commit and the owner's manual smoke test of the bundle were still pending, and the embedding-model comparison and Windows timing are unfinished.

## Local-only folders

Mark a folder **local only** and Lowk will never send its content to a cloud model: no answers, summaries, tags or cloud embeddings. If you connected a local LLM (Settings, providers: a local base URL and model for Ollama or LM Studio) it is used instead; otherwise the cloud route is refused and those buttons are disabled with an explanation. The block is enforced in the backend for answers, summaries, tags, retrieval and the cloud embedder. Note that choosing a cloud transcription engine still sends call audio to that engine, so use local transcription for sensitive calls.

## Stealth limits

The overlay is excluded from screen capture using OS window flags, and is hidden from the Dock, app switcher and tray. It does not appear in screenshots Lowk itself takes. Be aware:

- This relies on the sharing app respecting the OS flag. Many do; some capture paths (certain recording tools, capture cards, a phone filming the screen, remote-desktop or proxy setups, future OS changes) may still show it. **Test with your own meeting app and a second screen before you depend on it.**
- Lowk's process is visible in Activity Monitor or Task Manager. Hiding or disguising the process is not done (a rename and icon change is planned).
- The mouse cursor is visible on a shared screen. That is why moving the overlay uses a hold key and does not change the cursor.
- The main window is not stealth. Only the overlay is.
- Audio capture sounds nothing to the other side, but participants may be notified by the call platform that someone is recording, depending on its own rules.

## Consent

Recording or transcribing a call, and using an AI assistant during interviews or exams, may be regulated or forbidden by law, by your employer, a school or the meeting's rules. In many places all participants must consent. **You are responsible for knowing and following these rules.** The authors provide Lowk as is, without warranty, under AGPL-3.0-or-later, and are not liable for how you use it.
