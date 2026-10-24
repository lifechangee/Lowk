# M1 decisions: speech-to-text

## 1. Runtime for the live local engine (Nemotron 3.5 ASR Streaming Multilingual)

**Chosen: (c) ggml via transcribe.cpp (MIT), run in an out-of-process worker
(`workers/lowk-stt-worker`, not a workspace member).** One runtime serves both the live engine
(Nemotron cache-aware streaming, GGUF Q8_0, 751 MB), Silero VAD (1.2 MB) and the Whisper
large-v3-turbo correction pass (GGUF Q5_K_M, 620 MB). Metal on macOS, Vulkan/CUDA on Windows
(Cargo features of the worker). No Python anywhere. The worker is ~25 MB; models are never in
the installer.

Alternatives measured on the owner's MacBook Air M4 (machine heavily loaded by other agents, so
compare ratios, not absolutes), 42 s Russian clip, one stream:

| runtime | compute RTF | verdict |
|---|---|---|
| mlx-audio (Python, bake-off) | 0.14 | fastest, but needs Python |
| sherpa-onnx 1.13.8 (ORT) int8, CPU 2 / 4 threads | 1.82 / 1.09 | too slow |
| sherpa-onnx fp32 (2.4 GB), CPU or CoreML EP | 1.2 | too slow, 3x the download |
| FluidAudio CoreML (ANE) | n/a | model repo is gated behind a Discord invite |
| mlx-swift port | n/a | no maintained Nemotron implementation; weeks of work |
| **transcribe.cpp ggml Metal, Q8_0** | **0.35 (R=6) to 0.6 (R=13)**; 0.09 inside the worker | **chosen** |

Process isolation (docs 1.3): a crashed or hung worker is restarted by `SidecarEngine`
(3 restarts, then a non-recoverable `stt://error`); the overlay and stealth are unaffected.
The workspace never needs CMake or CUDA: CI keeps building only Rust.

Licence: the Nemotron weights are OpenMDW-1.1 (https://openmdw.ai/license/1-1/), shown before
the download (`EngineInfo.license_url`). Whisper and Silero are MIT.

## 2. Streaming behaviour

* Worker: 160 ms chunks, Silero VAD segments utterances (hangover 480 ms, cap 25 s), Nemotron
  cache-aware stream with `att_context_right = 6` (480 ms look-ahead). Language: one configured
  language forces its locale, several = the model's auto slot.
* Revisions: partials `rev 0..n`, final `rev n+1`, Whisper correction `rev n+2` of the same
  utterance. `TranscriptBuf` applies a strictly newer final over a final.
* `TranscriptBuf::snapshot_for_ask` includes the utterance still being spoken: Ask never waits
  for a final nor for a correction (tested).
* Each stream (Me, System) has its own worker process (~1 GB RAM each; ggml allows one active
  stream per loaded model).

## 3. Background correction (engine #2)

Whisper runs on finalized utterances only, as its own process in macOS background QoS
(`setpriority(PRIO_DARWIN_BG)`), 2 threads. Without the QoS the correction worker starved the
live streams (backlog grew to 24 s; with it the live latency is unchanged). Skipped when the
live engine's backlog exceeds 1 s, more than 4 jobs are queued, or the audio is <0.7 s or >29 s;
implausible output (length ratio outside 0.45..2.2) is dropped. The corrected text replaces the
stored text and the call journal gets only the better version (finals wait up to the 4 s stop
grace for it, never longer than the correction takes). Whisper writes digits ("42"), the
references spell numbers out, so CER under-states its gain.

## 4. Measured: full session path, owner recordings (20 clips, Me+System concurrently)

`cargo run --release -p lowk-session --example stt_replay` (FileAudioSource -> session ->
sidecar -> transcript -> bus; word-to-screen = first appearance on the bus vs estimated word end,
+-0.3 s, no forced aligner). Raw replay results are internal (not in this repository).

| config | word-to-screen p50 / p95 | mean CER live | mean CER after correction |
|---|---|---|---|
| Nemotron R=6 + Whisper (default) | 740 / 1907 ms | 0.269 | **0.172** |
| Nemotron R=3, no correction | 482 / 3763 ms | 0.284 | - |
| Deepgram Nova-3 multi (1 clip, them_q) | 349 / 1431 ms | 0.161 | - |
| Soniox | not measured: API returns 402 "Organization balance exhausted" | | |

The engine-side part (newest audio fed -> partial emitted) is ~35 ms p50; the rest is the
model's look-ahead (R x 80 ms + 160 ms chunk). **The p95 < 1 s target is not met** (p50 is);
the p95 tail is partly estimator noise and partly long monologues. R=3 trades 0.015 CER for
~260 ms lower median; it is a one-line tuning (`LocalTuning`), left at R=6 because the correction
pass repairs the accuracy anyway. To revisit with a forced aligner.

## 5. Cloud adapters

Soniox (default, `stt-rt-v5`) and Deepgram Nova-3 `language=multi` over blocking
`tungstenite` + rustls with the pure-Rust `graviola` crypto provider (no `ring`/aws-lc/openssl, so no C
toolchain and the Windows target cross-checks from any host; model downloads use `ureq` with the same provider); one connection per stream; reconnect x3; 4xx is fatal. Keys: env /
`~/.lowk-test-keys` (debug builds or `LOWK_DEV_KEYS=1`), else the keychain item `stt-cloud`;
never logged. Deepgram verified end to end on a real clip (word lag p50 349 ms in one run, 962 ms in another: network and estimator variance); Soniox authenticates and speaks the
protocol but the account has no balance, so transcripts were not verified.

## 6. Models on demand

`ModelStore` (`<app data>/models/<engine>/`): resumable (`Range`) downloads with retries,
SHA-256 pinned per file (HF LFS object ids), progress as `engine://progress`, size check on
start. `engine_status` returns fitness / installed / recommended / licence per engine
(recommendation = `hw::recommend`); `engine_install`, `engine_install_cancel`, `stt_diagnostics`
(p50/p95 per stream) are new commands. Until an engine is ready the record bind still plays the
scripted demo (`LOWK_DEMO=1` forces it).

## 7. Not done

Windows runtime untested (compile-checked; worker `vulkan`/`cuda` features unbuilt); worker
`hw_bench` stays a stub; no forced-alignment latency metric.

Known CI note: `just ci-cross` currently fails in `lowk-capture` (unused variable on Linux, clippy on `win.rs`), code merged from main and outside this work; the STT crates are clean on both targets.

## 8. Worker distribution and cold start (M1 QA fixes)

* **Worker is bundled**, not downloaded: 8.2 MB on macOS/Metal (the whole `Lowk.app` is 19 MB), so
  there is no pinned-hash download to maintain and no "models but no worker" state. `just build` =
  `tools/build-worker.sh` (builds the worker for the host triple, stages
  `crates/lowk-app/binaries/lowk-stt-worker-<triple>`) + `cargo tauri build --config
  tauri.bundle.conf.json --bundles app` (`externalBin`). The plain `tauri.conf.json` has no
  `externalBin`, so `cargo clippy/test` and `just dev` never need the staged file. Lookup order
  (`find_worker`): `LOWK_STT_WORKER`, next to the executable (= inside the bundle), `<app data>/engines/`,
  and in debug builds the in-repo build (`LOWK_NO_DEV_WORKER=1` disables that, QA uses it to prove the
  bundled lookup). Vulkan/CUDA stay optional worker features. CI (macOS) builds the bundle and runs the
  bundled worker; the Windows job compile-checks the CPU worker. If the worker is still missing the
  engine card says so (`EngineInfo.runtime_missing`).
* **Cold start**: the worker loads the model after Record (up to ~15 s on a cold disk cache, measured 17 s
  when the machine is busy). Two fixes. (1) `SidecarEngine::is_ready` + a session watcher publish
  `stt://loading {loading}`; the overlay shows «Загружаем модель распознавания…» (instead of «Слушаю…») and
  the main window a banner; subtitles start the moment the worker reports `ready`. Audio captured
  meanwhile waits in the engine queue (20 s) and the worker pipe and is transcribed after the load (the
  journal keeps the original timestamps). (2) 5 s after launch the app runs `lowk-stt-worker warm` once
  (live model + Silero, then Whisper if correction is on) at nice 19 / background QoS: it fills the OS file
  cache and compiles the GPU pipelines, then exits (no idle RAM). It is not started, or is killed, when
  free memory drops below 25 % (`kern.memorystatus_level`, macOS; elsewhere no pressure signal yet) and is
  skipped while a call is running. `LOWK_NO_WARMUP=1` disables it.

## 9. Per-call languages (BRIEF, last line) and the polish backlog

**Decision.** A call has a language set. Settings «Языки созвона» holds the default (`stt.languages`;
onboarding pre-fills system language + English, e.g. ru+en, en, de+en); the «Новый созвон» card and the header
popover show compact chips pre-filled with the last pick (`stt.last_languages`, written by `session_start`,
cleared when the default is changed in Settings). `StartOpts.languages` carries the pick; no pick (record bind)
= last pick, else default. The call frontmatter `languages` stores what was really used.

**What the set drives** (`lowk_types::langpolicy`, one std-only file; the STT worker includes the same file with
`#[path]`, so the app and the worker cannot drift):
* **Which languages are offered**: the model's own prompt table, read from the GGUF (`<xx-XX>` tokens): 34
  languages (ru en uk de fr es it pt nl pl cs sk sl hr sv da no fi hu ro tr lt lv et vi bg el he ar hi th ja ko
  zh). `EngineDescriptor.languages` = that list; the UI offers the chosen engine's languages; `resolve()` drops
  the rest again in the session (an unsupported or empty pick falls back to the default, never to nothing).
  Cloud: Soniox gets the set as `language_hints`, Deepgram `language=<one>` or `multi`.
* **Decoder hint**: one language -> its prompt (`ru-RU`, `pl-PL`, ...); several -> the auto slot plus the guard.
* **Allowed letters** (`is_foreign_letter`): ASCII always passes (names, terms, English words inside any call).
  Cyrillic needs a Cyrillic language in the set (`en` alone treats Cyrillic as foreign); і ї є ґ ў need `uk`/`be`;
  non-ASCII Latin letters pass only if a selected language writes them (`pl`: ąćęłńóśźż, `de`: äöüß, `tr`: çğıİöşü,
  ... `vi`: the whole Latin Extended Additional block); Greek/Hebrew/Arabic/Devanagari/Thai/Kana/Hangul/Han need
  their language. A drifted utterance is decoded again with the prompt of each selected language (at most 4,
  in the user's order), the cleanest result wins, words still foreign are dropped (S3 mechanics unchanged).
  The Ukrainian->Russian look-alike mapping for partials only applies when `ru` is selected and `uk`/`be` is not.
* **Whisper correction**: `corrector::accept_correction` = not damaged, no foreign letter for the call's set,
  changed, plausible. One language -> Whisper gets that language, several -> auto.

**Replay (files only, nothing played), 20 owner recordings, nemotron, no Whisper, `tools/qa/stt-lang-check.py ru,en ru en`**
(the tool maps a set to `--lang` via `decoder_lang` and to `--allowed`, counts letters outside the set, fails on any):

| set | mean CER | clips with Russian speech (18) | clips with English speech (2) | letters outside the set |
|---|---|---|---|---|
| ru+en | 0.2472 | 0.2449 | 0.2681 | 0 |
| ru only | 0.3606 | 0.2963 | 0.9391 | 0 |
| en only | 0.8118 | 0.8727 | 0.2632 | 0 (163 before the guard also ran with a single-language hint) |

Reading: a wrong or too narrow pick costs accuracy exactly where expected (English clips under ru-only, Russian
clips under en-only: the guard strips the other script instead of printing it), while the right pick is as good
as ru+en (English clips: 0.2632 en-only vs 0.2681 ru+en). A single-language decoder hint is only a prompt: under
`en-US` the model still wrote Cyrillic on Russian speech, so the guard now runs for every set.

**Polish backlog.** (1) The engine install card shows the real total: `EngineInfo.extra_mb` = the Whisper model
while background correction is on and it is missing; text «Загрузка: 1,4 ГБ (0,8 распознавание + 0,6 коррекция
Whisper)», the button says «Скачать (1,4 ГБ)», onboarding no longer hard-codes 1.6 GB, and the progress bar is
one cumulative bar over everything the install downloads. (2) Custom templates can be bound: `BindSpec.template`
(+ `ActionId::AskTemplate`, `BindEvent.template`, `BindStatusEntry.template`); slot identity is (action,
template). The key editor lists every template (built-in ones keep their dedicated actions), the same trigger
on two templates is a conflict (`duplicate of ...`, second one skipped), a bind of a deleted template stays
visible so it can be cleared and fails with `NotFound` rather than asking with another template.
The memory-pressure skip of the warm-up is still macOS only.

**Checks.** `langpolicy::tests` (per language set), `corrector::tests::correction_rule_follows_the_call_languages`,
`lowk-session` `call_languages_*` / `remembered_languages_*`, `cloud_ws::lang_tests`, matcher/conflict tests for
template binds, vitest (`langs.test.ts`, template binds), Playwright `e2e/m1-langs.spec.ts` (RU/EN, card, popover,
settings, onboarding, install total, template binds), `just qa-bundle` replays the BUNDLED worker for ru+en and
pl+en. `qa-bundle --silent` launches in the background and synthesizes no key presses (the lecture-safe mode).
