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
+-0.3 s, no forced aligner). Raw: `spikes/stt-bench/results/m1-session-replay-*.md`.

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
distribution (GitHub Releases + pinned hash) is still manual (`LOWK_STT_WORKER`, next to the
app, or `<app data>/engines/`); `hw_bench` stays a stub; no forced-alignment latency metric.

Known CI note: `just ci-cross` currently fails in `lowk-capture` (unused variable on Linux, clippy on `win.rs`), code merged from main and outside this work; the STT crates are clean on both targets.
