# Orchestrator smoke test of bundled Lowk.app (2026-10-21)

Setup: ad-hoc signed bundle (DR pinned to dev.lowk.app), all 5 permissions granted, nemotron-local + whisper installed. Source = afplay playing spikes/stt-bench/audio/01_them_q.m4a 3× in a loop through speakers (no headphones). Overlay is invisible to my screenshots (stealth), so overlay visuals are owner-verified only.

Works: real transcription (no demo), good RU text with EN terms in Latin (Backlog, Google Maps); all 8 binds active and firing (log); stop → main home; call .md written; permissions persist after relaunch.

Bugs (fix before owner hand-off):
| ID | Problem | Evidence |
|---|---|---|
| S1 | Text in the saved .md is cut mid-character → «�» (byte-boundary truncation of UTF-8) | call md lines «А, о�», «Расскажи ко�» |
| S2 | ⌘⇧S triggers ScreenshotArea; per BRIEF ⌘⇧S = all monitors, ⌘⇧A = area | lowk.log `bind event action=ScreenshotArea` on ⌘⇧S |
| S3 | Language drift to Ukrainian letters at utterance start («Підбегум от Sales», «З фідбиком») | call md first lines |
| S4 | Echo: without headphones System speech is transcribed again as «Я»; it also pollutes «Мой ответ» | md: same sentence as Система and Я; QA block |
| S5 | Source process restart → `TargetLost` and System capture stops for the rest of the call (real for apps that respawn audio helpers) | lowk.log `audio event other=TargetLost` |
| S6 | Timestamps: several lines stamped [00:00:00]; order/offsets look wrong vs audio | md |

## Fixes (2026-10-21, verified on the bundled app: `just build` ad-hoc, DR `identifier "dev.lowk.app"`, audio-input entitlement, real speakers + microphone; `just qa-bundle` 28 pass / 0 fail / 1 NEEDS-GRANT SystemAudio status)

| ID | Status | What / numbers | Permanent check |
|---|---|---|---|
| S1 | fixed + root cause found | The app never wrote «�»: all call files of the verification runs contain 0 U+FFFD bytes. The symptom came from byte-based viewing tools (`cut -c`, `head -c` with an empty `LANG` cut Cyrillic mid-character; reproduced). Hardened anyway: `lowk_types::text` (grapheme/byte-safe cuts, `clean_speech`), applied in the worker, sidecar parser, Whisper corrector (a damaged correction is rejected), session ingest and the call writer. | `text::tests` (property test: Cyrillic, emoji, ZWJ, flags, combining marks), `s1_*` in lowk-stt, lowk-store codec, worker |
| S2 | fixed | Defaults were already ⌘⇧S Screenshot (all monitors) / ⌘⇧A area (Windows Ctrl); the wrong log line came from the swapped/duplicated pair an older settings file could hold. `Settings::migrate_binds` repairs swapped or duplicated pairs on open and rewrites the file; custom binds are left alone. | `s2_*` in lowk-types and lowk-store; `just qa-bundle` starts from a swapped profile and presses the REAL ⌘⇧S / ⌘⇧A |
| S3 | fixed | Auto language slot drifts into Ukrainian letters. A single-language hint is wrong for ru+en users (ru-RU hint: CER 0.358, English clips 0.67-0.97). Instead the worker gets `--allowed ru,en`; a final with letters outside the user's languages (і ї є ґ ў, accented Latin when no such language is allowed) is decoded again from the same audio with the ru-RU prompt. 20 recordings (spikes/stt-bench), nemotron, no Whisper: CER 0.2620 -> 0.2605, Ukrainian letters 20 -> 0. | `s3_*` in the worker; CER script numbers above |
| S4 | fixed | WebRTC AEC3 now ships in macOS bundles (`just build` passes `--features aec`, meson/ninja via uv). Auto mode: on unless headphones. Safety net `lowk-session::echo`: a Me sentence that fuzzy-matches System speech around the same audio time is dropped (per sentence, so real double-talk survives). 155 s of System speech through the speakers, one afplay: Me sentences that are echo duplicates: AEC off + guard off 32 of 33; guard only 5 of 7 (121 partial/final events dropped, 15 trimmed); AEC only 0 of 0; AEC + guard 0, and two real utterances spoken by `say` over the same speakers were kept. «Мой ответ» excludes echo (test). | `echo::tests`, `s4_*` in lowk-session, `qa-bundle` checks AEC3 is linked |
| S5 | fixed | A pid/CLI target is now remembered by identity (bundle id or executable). New pid within 1.5 s: tap rebuilt immediately, no gap in the call. 6 s gap: log + `audio://notice` toast in the overlay, cleared when the app is back (`capture target re-acquired`, `audio target restored`). | `s5_*` in lowk-audio (procs) and lowk-session; overlay vitest |
| S6 | fixed | (a) Capture time of a buffer ignores garbage HAL host times (they could pin a stream to ~0 = `[00:00:00]`), (b) the call file is ordered by audio time: a Me final arriving after a later System line or a Q&A block goes back where it was spoken. | `s6_*` in lowk-audio tap and lowk-store codec |

Real-audio runs (settings.toml of the owner restored afterwards, test calls moved out of the vault): before = AEC off, `LOWK_ECHO_GUARD=off` (diagnostic env var, bypasses only the transcript guard).
