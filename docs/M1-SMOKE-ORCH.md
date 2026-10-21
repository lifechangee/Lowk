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
