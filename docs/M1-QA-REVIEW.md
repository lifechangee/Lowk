# M1 QA review (independent)

Reviewer ran everything on the owner's MacBook Air M4 with **no audio output** (file sources only). Evidence: `docs/qa/m1/` (results.json, live-call.md, live-events.json), `spikes/stt-bench/results/m1-qa-*.md`, `tools/qa/m1-e2e.mjs`, `docs/qa/report.html`.

## 1. Gates

| item | result |
|---|---|
| `just ci` | PASS (after two fixes below) |
| `just qa` UI suite (chromium+webkit) | PASS, 595 tests (local pixel baselines were stale: 44 diffs until re-recorded) |
| `just qa` real app | first run 30/30 PASS; later runs FAIL 6 ScreenCaptureKit checks with -3811 because the display went black/asleep (screencapture returns a black frame). Environment, not code: re-run with the screen on |
| GitHub CI for HEAD c2316dd | green at start (a run was in progress); new commit checked at the end |
| `cargo deny` | FAIL on current main: RUSTSEC-2026-0381 (webpki-roots 0.26.11 via tungstenite). FIXED: tungstenite now uses our own rustls connector with webpki-roots 1 |

## 2. Latency (20 recordings, full session path, warm, machine load ~2-3)

| config | word-to-screen p50 / p95 | exact-anchor (last word) p50 / p95 | CER live / corrected |
|---|---|---|---|
| R=6 + Whisper (old default) | 718 / 1692 | 517 / 1263 | 0.269 / 0.180 |
| R=6, no correction | 571 / 1541 | 368 / 784 | 0.269 |
| R=3, no correction | 428 / 1392 | 273 / 736 | 0.284 |
| **R=3 + Whisper** | **435 / 1390** | 279 / 771 | 0.284 / **0.170** |

* Claimed numbers reproduced (740/1907 vs 752/2487 first run, 718/1692 warm). The first run had 14-17 s lags in session 01 only: **cold start**. The worker loads the model after the record bind already reports Recording and audio queues (no "loading" state). Warm rerun of the same clip: p95 1.5-1.7 s. Real cost for the first call after boot/rebuild; not fixed (see issues).
* p95 tail: partly estimator noise (12% of words have negative lag; middle words are placed by character share), partly real. The exact anchor (end of utterance) gives p95 0.74-1.26 s. **p95 < 1 s is not demonstrated; p50 is met.**
* Correction does slow the live path: +150 ms p50, +300-400 ms p95 on R=6 (shared GPU/CPU) despite background QoS (correction worker verified nice 19). Not a block of live partials, but not free. At R=3 the cost vanishes into the noise (435 vs 428).
* `ask()` during replay (305 probes, 2.3 s apart): p50 0.12 ms, max 0.38 ms with correction (0.20 without): never blocked. 
* **Recommendation: default R=3** (done in `LocalTuning`): -280 ms p50 vs R=6 with correction, and better corrected CER (0.170 vs 0.180).

## 3. Real app, file source as System (and Me) stream (`m1-e2e.mjs`, 14/14 PASS after fixes)

| item | result | evidence |
|---|---|---|
| live subtitles in overlay, both speakers | PASS (first line 4.7 s after Record incl. 3 s of speech lead-in) | live-events.json |
| finals in call .md, `[hh:mm:ss] **Я/Система:**` | PASS. Cosmetic: lines ordered by finalization, a Я line stamped 00:00:03 follows a 00:00:07 line | live-call.md |
| background correction revises stored text | PASS 12/12 stored corrected | live-events.json |
| Ask on a partial includes it at once | PASS (question = "Що с…" while System was mid-sentence). **Was FAIL**: question fell back to the previous final; fixed | live-call.md |
| «Мой ответ» closes only on next Ask | PASS (grows 0→169 chars across System speech, resets at ask #2, saved in .md). **Was FAIL**: speech during hint streaming was lost, and the close update re-showed the old text under the new question; both fixed | e2e log |
| stop → main window home | PASS (idle 340 ms) | results.json |
| no engine: record bind | PARTIAL→PASS: plays the demo; **was silent, no install UI existed at all**. Added Settings > Providers model card (status, size, licence, progress, cancel, retry, reasons) saying that the record button plays a demo until installed | results.json |
| install: progress / cancel / resume / SHA mismatch | PASS in the real app against Hugging Face (resumed 21.1→23.4 MB; full-size corrupt file rejected with message). **Was FAIL** for a complete `.part` (416 loop, stuck forever); fixed | results.json |

Also found and fixed: live page duplicated lines when correction re-emitted an utterance; overlay subtitle jumped to old sentences when a correction landed; STT errors were never shown (overlay now shows restarting/stopped); Providers page showed no engine when the setting was empty.

## 4. Round 2 in the real app
`just qa` real-app (first run, all PASS): G1 size presets overlay+main, G2 pill drag + remembered position, G3 picker (presets, no ids), G5 no duplicate button/popover, G6 per-template ask ⌘⇧↵, hold-⌃ 20x clickable with 0 absent frames, G7 queue per monitor with own windows excluded. Not machine-verified: installed-apps list contents in "Other…" (real list not inspected), **⌘⇧I typing with real keystrokes** (not run: no keystroke injection), G4 grant prompt. Re-run after screen wake for the SCK-dependent ones.

## 5. Remaining issues
* ~~HIGH: worker distribution~~ FIXED: bundled as a Tauri sidecar (`just build`), card shows `runtime_missing` (DECISIONS-M1 §8).
* ~~HIGH: cold start~~ FIXED: background warm-up after launch + «Загружаем модель распознавания…» state in overlay and main window; audio is buffered (DECISIONS-M1 §8).
* MED: p95 word-to-screen ~1.4 s (target 1 s); correction adds latency at R=6.
* MED: `installed()` checks file size only; a same-size corrupt model is "ready".
* MED: a crash loses finals pending correction (journal waits for the better text, a few seconds).
* LOW: after two Asks during one long utterance the second omits its continuation (llm cursor). «Мой ответ» appends the whole straddling utterance. Journal line order by finalization.
* LOW: `just ci-cross`/Windows runtime still untested.

## 6. Owner should try by hand
1. ~~Audible: per-app isolation~~ done, see «Тесты со звуком» (still worth a try with real YouTube + Discord).
2. Audible: a real call in Discord/Zoom with a headset and with speakers (echo, see big item below).
3. Settings > Providers: download the model on a fresh profile; kill Wi-Fi mid-download and retry.
4. ⌘⇧I typing in the overlay; ⌘⇧S screenshot with a real second display.
5. First Record right after a reboot: watch for a delayed start.

## M1 polish backlog
- Engine install card shows 0.8 GB, but with Whisper correction the total download is ~1.4 GB — show the real total (or list both models).
- Memory-pressure skip for warm-up exists on macOS only.

## Тесты со звуком

Owner allowed sound. Setup: MacBook Air M4, built-in speakers + mic, volume left as the owner had it (19 %, unmuted for the final runs; the first runs happened with output muted, which the per-app/system taps do not care about but the mic does). Tools: `tools/audio/isolation.sh`, `tools/qa/m1-e2e.mjs --stage audible-app,audible-system` (new, opt-in), `capture` + `stt_replay`. Evidence: `docs/qa/m1/audible-*`.

Environment gotcha: with the **display asleep** (nobody at the Mac) `afplay` fails with `AudioQueueStart failed (-66681)`. `caffeinate -d -u` wakes it; the audible harness needs an awake display.

### 1. Per-app isolation: PASS
* `isolation.sh`: app=440 player 0.300 / 0.0001, app=880 player 0.0000 / 0.2999, whole system both 0.30, 440 stopped 0.0001 / 0.3000.
* Realistic: afplay A = `02_them_q` (the call), afplay B = `03_them_q` (another conversation), same time. `--app A` transcript is only A (question about ARR / burn rate / runway, nothing of стендап / деплой / блокеры); `--system` has both. See `audible-isolation-transcripts.md`. Not done: Chrome/YouTube (no audio tab available, nothing logged into).

### 2. Real app, source = one app (afplay), mic on, qa build: PASS 13/13 (`audible-app-*`)
* Live subtitles in the overlay from 5.1 s after Record (4 s of silent lead-in in the clip), both speakers; screenshots `audible-app-live-subtitle.png`, `-after-ask2.png`.
* Finals in the call .md with correct «Система» lines: **CER 0.150 / WER 0.203** vs the reference text (after Whisper correction), `audible-app-live-call.md`. The own-voice clip played by a second process (reaches the real mic only) contributes 0 bigrams to System.
* Latency from real capture (17 utterance ends): speech end → first text on screen **p50 106 ms / p95 441 ms**; speech end → final **p50 836 / p95 1846 ms**; newest audio sample → emitted text p50 43 ms (p95 ~1.4 s: whole-utterance re-emits). Unlike the file replay these include real capture (ring + resampler 7 ms) but the estimator is exact (utterance ends found from the wav).
* ⌘↵ mid-question: question stored = the live partial («...Какую у вас сейчас AR»), PASS; «Мой ответ» collects from the real mic and resets on the next Ask, saved in the .md; stop → idle 335 ms → main window on home.
* `audible-system-*` (whole system, 3 clean runs): CER 0.20-0.23, WER 0.30-0.38.

### 3. Whole-system mode: PASS after a fix
The app played a different clip (`04_them_q`) inside its own webview (WebAudio) during the capture. **Before the fix it leaked into System** (18/25 distinctive words, 26 bigrams). Cause: `own_tree_objs` only excludes processes whose *responsible pid* is us; a binary started from a terminal/harness (`just dev`, QA) is not the responsible process of its WebKit helpers (checked: both resolve to the launching host), so the helpers were captured. Fixed in `crates/lowk-audio/src/mac/procs.rs`: when we are not our own responsible process, the WebKit XPC helpers of the same host started after us are excluded too. After: 0/25 words, 0 bigrams, 4 of 4 runs. A Finder/launchd-started bundle was not affected (it is responsible for its helpers); not testable by the harness.

### 4. Echo (speakers + mic): BIG, open
With the call coming out of the built-in speakers at 19 % volume, System audio **does come back as «Я»**: 1 of 4 «Я» lines is a complete copy of the first System sentence (`Спасибо, что нашел время... ARR?`), two more «Я» lines carry System fragments («4 месяца это мало», «вчерашний диплом откатывали», «нужна помощь с API для мобильного клиента») mixed into the owner's real answer. `aec: auto` does not remove it enough on this Mac. Effect: duplicated text in the journal and in the LLM context, and the owner's answer polluted with the interviewer's words. With headphones there is no echo. Suggest: check the AEC reference path (is the tapped per-app stream the AEC far-end?), and as a mitigation drop a «Я» final whose words are mostly contained in a System final from the last ~3 s. Own-voice CER on «Я» is 0.55-0.67 (echo + room + the 02_me clip played at low volume), so «Я» quality here is not a model number.

### Small defects fixed
* Main window stayed on the previous route after Stop in 3 of 5 early audible runs (home reached only after the idle poll): `library.reload()` failing made `router.go(home)` never run. Now home is opened even if the reload fails (`ui/src/main/state.svelte.ts`). After the fix 1 of 9 runs still showed it once (not reproduced in the last 8); the harness now records the `session://ended` events to catch it.
* Whole-system CER dropped to 0.55 in one run when the owner changed volume/mute mid-run (the second clip is missing from the .md): capture of the whole-system tap seems to stall after an output device property change. Not reproduced in 5 later runs, not investigated.

### Not covered
* Discord/Zoom/YouTube real apps; headset case; a second run of the echo measurement with different volumes.
- CRASH (fixed 2026-10-21): bundled Lowk.app had no NSMicrophoneUsageDescription / NSAudioCaptureUsageDescription → macOS killed the app on the mic request in onboarding (TCC abort). Dev mode hid it (permissions inherited from the host app). Follow-up: add a CI/qa check that the built bundle's Info.plist contains every privacy key we request, and run real-app qa against the bundled .app, not only dev builds.
