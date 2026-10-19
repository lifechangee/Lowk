# Lowk: real-time STT research (2026-10-19)

Scope: speech-to-text with < 1 s latency from speech to text on screen, any language, code-switching (RU with EN terms), two streams (mic = "me", system audio = "them"). Targets: macOS 14+ on Apple Silicon, Windows 10/11.

Method and caveats. I read primary pages (vendor docs, model cards, GitHub READMEs) wherever I could and used secondary sources otherwise. Every claim is marked **[P]** (primary) or **[S]** (secondary or vendor-marketing; verify). No benchmarks were run. Latency and throughput figures for Whisper on specific chips are mostly estimates and are labelled as such. Section 9 lists the benchmarks to run before locking in the defaults.
ElevenLabs pages redirected my fetcher to a geo-restriction help article, so ElevenLabs data is secondary only. (Worth knowing for a RU-facing product: check ElevenLabs regional availability.)

---

## 0. TL;DR

| Platform | Default local engine | Fallback |
|---|---|---|
| Mac (M1+, >= 16 GB ideal, 8 GB OK for small models) | **Nemotron 3.5 ASR Streaming Multilingual 0.6B** (MLX 5-bit or CoreML INT8, ~540-610 MB) for true streaming; **Voxtral Realtime 4B (MLX 6-bit/8-bit)** as the higher-quality option on M2 Pro/Max and up | Whisper large-v3-turbo (whisper.cpp + Metal/CoreML) with the streaming wrapper in section 1.4 |
| Windows + NVIDIA GPU | **Nemotron 3.5 streaming** (ONNX Runtime CUDA or NeMo-Speech.cpp) and/or **Voxtral Realtime** via a vLLM-free runtime, once one is mature (see risks); **Whisper large-v3-turbo via whisper.cpp CUDA** is the safe, well-trodden path | Cloud |
| Windows CPU / iGPU | **Nemotron 3.5 streaming (INT8 ONNX/GGUF Q8)** on CPU if the benchmark passes; otherwise **cloud** | Whisper small/base is not recommended (accuracy) |
| Cloud default | **Soniox stt-rt-v5** ($0.12/h, 60+ languages, per-token language tag, one multilingual model) | Deepgram Nova-3/Flux Multilingual (Russian + EN code-switching documented, $0.35-0.47/h), OpenAI gpt-live-transcribe, Mistral Voxtral Realtime API ($0.36/h) |

Whisper stays as a **selectable "Whisper (accurate, slower)" engine**. Its architecture is not streaming, so it gives about 1.5-3 s latency on partials with the best wrapper and cannot honestly promise < 1 s on most machines. Pitch it as "highest language coverage, final-quality text", not as the realtime default.

OpenRouter: has STT (`/api/v1/audio/transcriptions`) but **no streaming/WebSocket STT**. It cannot be the realtime path. Detail in section 4.1.

---

## 1. Whisper large-v3-turbo used in streaming fashion

### 1.1 Why it is inherently not realtime
Whisper is an encoder-decoder trained on 30 s windows. Even with turbo (4 decoder layers instead of 32, ~809M params), the encoder always processes a padded 30 s window (or a shortened one in some forks). Streaming is faked by re-running on a growing buffer and agreeing on a stable prefix. NVIDIA's own Nemotron write-up describes the official Whisper method as sliding 30-second windows that is "not native low-latency streaming" [S: wavect review](https://wavect.io/blog/nvidia-nemotron-3-5-asr-production-review/).

### 1.2 Approaches
| Approach | How it works | Latency (published or estimated) | Notes |
|---|---|---|---|
| **whisper_streaming (LocalAgreement-n)** | Re-transcribe growing buffer; commit a prefix once n consecutive hypotheses agree; trim buffer at segment boundaries | Paper reports **3.3 s** average latency on unsegmented long-form [P: README](https://github.com/ufal/whisper_streaming) | README says it is outdated in 2025, replaced by SimulStreaming. Backends: faster-whisper (recommended), whisper-timestamped, OpenAI API, mlx-whisper |
| **SimulStreaming** (same author) | AlignAtt simultaneous policy on Whisper decoder; MIT | Better than LocalAgreement per README claim [P](https://github.com/ufal/whisper_streaming) | Torch-only backend: heavy to ship in a light Tauri app |
| **whisper.cpp `whisper-stream` example** | Sliding window over mic audio, SDL2 | ~1-3 s typical (my estimate) | Example, not a library API; we would reimplement the loop on `whisper-rs` [P](https://github.com/ggml-org/whisper.cpp) |
| **VAD-chunked "pseudo-streaming" (recommended)** | Silero VAD cuts at pauses; each utterance is decoded as soon as it ends; during speech, re-decode the open utterance every ~500-700 ms on the last N seconds for a *partial*; on end-of-speech emit *final* | Partial: ~0.7-1.5 s behind speech on a good GPU/Mac; final: ~0.3-1 s after pause | whisper.cpp supports `--vad` with a Silero model [P](https://github.com/ggml-org/whisper.cpp) |
| **WhisperKit (Argmax)** | Swift/CoreML, macOS 14+, MIT; CLI `--stream` mic mode; real-time transcription *with speakers* is in the paid Pro SDK | Not published on the page | **Apple only, no Windows** [P](https://github.com/argmaxinc/WhisperKit). Models include `large-v3-v20240930_turbo` and a 626 MB compressed variant |
| **mlx-whisper** | MLX port, Apple Silicon | One tiny community timing: whisper.cpp q5_0 + CoreML 1.23 s vs mlx-whisper 1.02 s on a short clip, chip unstated [S](https://github.com/anvanvan/mac-whisper-speedtest) | Python; poor fit for Tauri unless sidecar |
| **faster-whisper (CTranslate2)** | CUDA/CPU; no Metal | Fast on NVIDIA | Python sidecar; one source puts RTX 4070 large-v3 at ~12x realtime vs whisper.cpp CUDA ~8x [S](https://www.promptquorum.com/power-local-llm/local-whisper-stt-comparison-2026) (unverified) |

### 1.3 Realistic numbers (state of evidence)
I could not find a trustworthy per-chip table for large-v3-turbo.
- Sources I found: a 2026 guide claims turbo "balances speed + accuracy at 14-18x" realtime and M1 large-v3 at 2-3x [S: unverified, ambiguous table](https://www.promptquorum.com/local-llms/apple-silicon-whisper-metal-benchmark). whisper.cpp with Core ML encoder advertises >3x encoder speedup vs CPU [P](https://github.com/ggml-org/whisper.cpp).
- Working assumption for planning (to be validated by the benchmark in section 9): turbo q5_0 on M1/M2 base chips transcribes a 5-8 s utterance in roughly 0.5-1.5 s; M3/M4 Pro/Max is faster. Windows with RTX 3060+ CUDA is comfortably faster than realtime; Windows with Vulkan on AMD/Intel iGPU is "works, performance depends on GPU and driver", no numbers found [S](https://github.com/DomoticX/whisper.cpp-windows-vulkan); CPU-only turbo is not realistic for < 1 s.
- Consequence: the "re-decode every 500 ms" partial loop costs a full decode each time. On an 8 GB base M1, running the loop for two streams (mic + system) at once will likely exceed realtime. Treat Whisper as one-stream-at-a-time capable on low-end Macs.

### 1.4 Recommended way to make Whisper "near-realtime" (since the owner insists)
1. Silero VAD (via whisper.cpp VAD or ONNX) per stream, 30 ms frames. Utterance start at speech onset; hard cap an utterance at ~12-15 s (force a cut at the lowest-energy point).
2. **Partials**: every ~600 ms while the utterance is open, decode the open utterance (not 30 s of padding; pass the real length, whisper.cpp supports `audio_ctx` shortening, which trades some accuracy for speed). Show partial text in a grey/italic style. Apply LocalAgreement-2 over successive partials so the displayed prefix does not flicker.
3. **Finals**: on VAD end-of-speech (~300-400 ms hangover), do one final decode of the whole utterance with a short previous-text prompt for continuity; replace partials with final.
4. Set `language` to auto only for the first utterance in a session and when confidence is low; otherwise supply a language list/hint to avoid language flapping. See code-switching below.
5. Suppress hallucinations on silence (VAD gating is mandatory; known Whisper failure on quiet/short segments).
6. Quantize: q5_0 or q8_0 turbo (model file ~0.55-0.9 GB; full fp16 ~1.6 GB). Core ML encoder on Mac.
Expected user-visible behavior: first text ~0.8-1.5 s after speech onset on good hardware, final text ~0.3-0.6 s after pause. Marketing claim "< 1 s" is only defensible for finals on strong hardware; say so honestly in the UI ("accurate mode").

### 1.5 Code-switching and Russian
- No benchmark of large-v3-turbo on RU/EN code-switching found. Evidence is indirect: a study on Arabic code-switching found letting Whisper auto-detect beats forcing one language [P: arXiv](https://arxiv.org/pdf/2410.04527). Whisper language detection is per 30 s window/segment, so mid-sentence switches rely on the decoder being tolerant; short segments hallucinate [S](https://convertaudiototext.com/blog/fix-multilingual-code-switching).
- A community LoRA of turbo for Russian with English tech terms claims WER 24.6 -> 9.1% on its own set (self-reported, TTS-based data) [S](https://friendli.ai/models/coriollon/whisper-large-v3-turbo-russian-codeswitch). Interesting for an optional fine-tune; check licence and quality.
- Practical mitigation: do not force `language=ru`; pass an initial prompt containing typical Latin-script terms (EBITDA, KPI, sprint) to bias spelling.

---

## 2. Voxtral (Mistral)

- **Voxtral Mini 4B Realtime 2602**, released 2026-02-04, **Apache 2.0**, ~4B params (~3.4B LM + ~970M audio encoder, BF16 safetensors) [P: model card](https://huggingface.co/mistralai/Voxtral-Mini-4B-Realtime-2602).
- Natively streaming (not chunked offline). Delay configurable 80 ms to 2.4 s; recommended **480 ms**; at 480 ms WER is within about 1-2% of the offline Voxtral Mini Transcribe 2; at 2.4 s it matches it [P](https://mistral.ai/news/voxtral-transcribe-2).
- **13 languages**: EN, ZH, HI, ES, AR, FR, PT, **RU**, DE, JA, KO, IT, NL. FLEURS avg WER 8.72% at 480 ms; **Russian 6.02%** [P: model card](https://huggingface.co/mistralai/Voxtral-Mini-4B-Realtime-2602).
- Code-switching: **not documented** by Mistral; test required. Language set is closed at 13 (no Ukrainian, Polish, etc.), which conflicts with "any language".
- Cloud API: `voxtral-mini-transcribe-realtime-2602`, **$0.006/min = $0.36/h** [P](https://mistral.ai/news/voxtral-transcribe-2); not compatible with diarization [P](https://docs.mistral.ai/studio/audio).
- Local runtimes: vLLM is the only officially supported one (needs a GPU with >= 16 GB; effectively Linux/CUDA, so not a desktop-app path). Community ports: pure C, MLX (mlx-audio), Rust, Swift (`mzbac/voxtral-mini.swift`, macOS 14+), llama.cpp support requested but not official [P: model card](https://huggingface.co/mistralai/Voxtral-Mini-4B-Realtime-2602), [P](https://github.com/mzbac/voxtral-mini.swift).
- Apple Silicon measurements (community, one M5 Pro 64 GB): **fp16 ran at 0.47x realtime (too slow)**; 6-bit/8-bit keep up; 6-bit first text ~1.8 s vs 5.4 s fp16; WER 8.6% vs 9.7% [S](https://huggingface.co/flo5k5/Voxtral-Mini-4B-Realtime-2602-MLX-6bit). So a 4B model needs 6-bit MLX and a Pro-class chip; base M1 8 GB is unlikely to work. Quantized weights ~3 GB.
- Windows local: no mature non-vLLM runtime found. Treat as Mac-only/NVIDIA-experimental for now.

Verdict: best quality/size trade-off among local streaming models for Russian on strong Macs; poor language coverage; runtime maturity is the risk.

---

## 3. Other streaming-native local models

| Model | Streaming | Languages | Russian | Size | License | Runtimes | Notes |
|---|---|---|---|---|---|---|---|
| **NVIDIA Nemotron 3.5 ASR Streaming Multilingual 0.6B** (cache-aware FastConformer-RNNT) | Native; chunks 80/160/320(default)/560/1120 ms | 40 locales; only 32 usable out of the box | Yes, in the "transcription-ready" tier | 0.6B; CoreML INT8 612 MB, MLX 5-bit 539 MB, MLX 4-bit 473 MB, ONNX INT8 ~650 MB | OpenMDW-1.1 (permissive, commercial OK; verify on model card; the HF card returned 401 to my fetcher) | NeMo, Transformers 5.13, NeMo-Speech.cpp (C++, Q8_0 GGUF, WebSocket), ONNX Runtime via `parakeet-rs`, MLX/CoreML bundles | FLEURS avg 10.4% (ready tier) at 80 ms with language known; auto language detect is worse (e.g. HI 11.5% vs 8.1%); emits `<lang>` tag per utterance. No Russian-specific WER or code-switching numbers published. Peak RAM ~0.75-1.5 GB on Apple Silicon. Sources: [S](https://wavect.io/blog/nvidia-nemotron-3-5-asr-production-review/), [S](https://soniqo.audio/guides/nemotron), [S](https://openwhispr.com/blog/parakeet-vs-whisper-vs-nemotron) |
| **Parakeet TDT 0.6B v3** | Offline/batch (buffered pseudo-streaming possible) | 25 European langs | Yes | ~680 MB INT8 | CC-BY-4.0 (to verify) | ONNX, NeMo, sherpa-onnx | Very fast on CPU; English WER ~6.3% [S](https://openwhispr.com/blog/parakeet-vs-whisper-vs-nemotron). Good *final-pass* engine |
| **Kyutai STT** (1B en/fr, 2.6B en) | Native, 0.5 s delay (1B) | EN, FR only | No | 1B / 2.6B | Weights CC-BY 4.0, code MIT/Apache | MLX (Mac), Rust server | Excellent tech, **wrong languages** for us [P](https://github.com/kyutai-labs/delayed-streams-modeling) |
| **Moonshine (Useful Sensors / moonshine-ai)** | Streaming-first models, any length | Multiple (UBOS blog mentions AR, JA, KO, ES, VI) | **Not confirmed**; non-English legacy models are non-commercial licensed | tiny to base | MIT for main models; non-commercial community licence for legacy non-English | C API, Python/JS/iOS/Android/Win/Mac/Linux | Verify language list at its docs before considering [P](https://github.com/moonshine-ai/moonshine), [S](https://ubos.tech/?p=63778) |
| **Voxtral Realtime** | Native | 13 | Yes | 4B | Apache 2.0 | see section 2 | |
| **Whisper turbo** | Pseudo | 99 | Yes | 809M | MIT | many | section 1 |

Conclusion: **Nemotron 3.5 multilingual is the only small, permissively licensed, truly streaming, Russian-capable model** that runs on both Mac and Windows CPU. Its multilingual WER is mediocre vs cloud; code-switching quality is unknown and must be tested early.

---

## 4. Cloud realtime STT

### 4.1 OpenRouter
- Has a STT endpoint `POST /api/v1/audio/transcriptions` (batch). Models: openai/whisper-1, whisper-large-v3, gpt-4o-transcribe, microsoft MAI-Transcribe, Groq Whisper, Google Chirp 3. Multipart limit 25 MB, **upstream timeout 60 s**; the doc describes a single request/response call and does not mention streaming or WebSocket [P](https://openrouter.ai/docs/guides/overview/multimodal/stt), [S](https://openrouter.ai/blog/announcements/announcing-audio-apis).
- Verdict: **no realtime/streaming STT on OpenRouter.** Possible only as a *"re-transcribe finals"* quality pass (chunks of 5-20 s via Groq Whisper), with 1-3 s latency; not an engine for the live view. Keep OpenRouter for the LLM answers.

### 4.2 Comparison (cloud)
Prices are pay-as-you-go list per hour; verify before shipping (several vendors run promos).

| Provider / model | Latency (claimed) | Languages | RU + EN code-switching | $/hour | WebSocket API | Source quality |
|---|---|---|---|---|---|---|
| **Soniox stt-rt-v5** | Low-latency; non-final tokens immediately; endpoint detection finalizes early. No ms figure published | 60+, one multilingual model | Per-token `language` field with language ID; `language_hints` accepted, "accuracy can degrade when LID is ambiguous"; no explicit code-switching claim on official pages | **$0.12** (diarization, LID, timestamps included) | Yes, `wss://stt-rt.soniox.com/transcribe-websocket`, `is_final` per token | [P pricing](https://soniox.com/pricing), [P docs](https://soniox.com/docs/stt/rt/real-time-transcription), [S](https://soniox.com/compare/stt/pricing) |
| **Deepgram Nova-3 Multilingual** | ~300 ms class (not re-verified) | `multi` = EN, ES, FR, DE, HI, **RU**, PT, JA, IT, NL (10) with real-time code-switching | **Yes, documented** (RU in list) | **$0.35** (PAYG $0.0058/min; promo rate) + $0.08 keyterms | Yes | [P](https://developers.deepgram.com/docs/models-languages-overview), [P](https://deepgram.com/pricing) |
| **Deepgram Flux Multilingual** | Turn-based, built for voice agents | same 10 | yes | $0.47 ($0.0078/min) | Yes | [P](https://deepgram.com/pricing) |
| **AssemblyAI Universal-Streaming Multilingual** | ~300 ms class (not verified) | EN/ES/DE/FR/PT/IT per marketing; **Russian not confirmed for streaming** | no | $0.15 (Universal-3.6 Pro Realtime $0.45) | Yes | [P](https://www.assemblyai.com/pricing.md) (languages unresolved) |
| **ElevenLabs Scribe v2 Realtime** | ~150 ms (claimed, excl. network) | 90+ claimed, conflicting (30-99) | not verified | $0.28-0.39 | Yes, with VAD and manual commit | [S only](https://elevenlabs.io/realtime-speech-to-text) (my fetch geo-redirected) |
| **OpenAI Realtime transcription** (`gpt-live-transcribe`, `gpt-transcribe`) | Tunable `delay` (lower = earlier partials, worse accuracy); no ms figure | broad; `languages` list param supported for multilingual | Supported via `languages`; `gpt-transcribe` returns detected languages | gpt-realtime-whisper listed ~$0.017/min = ~$1.02/h [S: third-party, unverified] | WebSocket (server) or WebRTC; no server_vad: client must commit turns; no word timestamps | [P docs](https://developers.openai.com/api/docs/guides/realtime-transcription), [S price](https://costgoat.com/pricing/openai-transcription) |
| **Gladia Solaria-1** | ~103 ms best-case partial, ~270 ms avg (vendor) | 100+ | Explicit code-switching feature with per-word language labels | $0.75 starter, $0.25 at 10k h/mo commit (vendor blog) | Yes | [S vendor](https://www.gladia.io/product/audio-intelligence/code-switching) |
| **Speechmatics** | low (vendor) | 55+ | **No: official pricing page says neither Standard nor Enhanced supports mixed-language/code-switching in real-time** | Standard $0.45, Enhanced $0.80 (opt-in training discounts 0.30/0.54); $100 free credits | Yes | [P](https://www.speechmatics.com/pricing.md) |
| **Mistral Voxtral Realtime API** | configurable 80 ms-2.4 s | 13 | untested | $0.36 | Yes | [P](https://mistral.ai/news/voxtral-transcribe-2) |

### 4.3 Reading the table
- Cheapest and broadest for our case: **Soniox** at $0.12/h. A 6 h/day heavy user ~ $0.72/day per stream; two streams double it (the "me" stream could be free local, system stream cloud).
- The only vendor with *officially documented* Russian + English streaming code-switching and a stable public price: **Deepgram Nova-3 `multi`**. Two-language-only constraint (10 languages in the code-switch set).
- Speechmatics is ruled out for code-switching. AssemblyAI is unproven for Russian streaming.
- Gladia is attractive on features but pricing/latency is vendor-reported and starter price is high.
- Recommendation: default **Soniox**, offer **Deepgram** as an alternative and as a fallback if a code-switching bake-off shows Soniox weak. Because Lowk is BYOK in the MVP, build a small `SttProvider` trait with WebSocket adapters; Soniox and Deepgram are both simple JSON/binary WebSocket protocols.
- Privacy note for BYOK/cloud: audio of third parties ("them") leaves the device. This needs an explicit consent line in the onboarding legal warning.

---

## 5. Hardware auto-detection (first launch)

Goal: decide per machine which local engine, if any, sustains realtime for both streams, and fall back to cloud otherwise.

1. **Static pre-filter (instant)**
   - Mac: arch must be arm64; `sysctl hw.memsize`, chip brand string (`machdep.cpu.brand_string`), `hw.perflevel0.physicalcpu`. Rough tiers: 8 GB base chips -> Nemotron only or cloud; 16 GB+ -> Nemotron + Whisper turbo q5; 16 GB+ with Pro/Max GPU cores >= 16 -> Voxtral 6-bit eligible.
   - Windows: RAM (`GlobalMemoryStatusEx`), CPU cores/AVX2 flag, GPUs via DXGI (adapter vendor, dedicated VRAM). NVIDIA: query NVML or `nvidia-smi`; require VRAM >= 4 GB for Whisper turbo q5 / Nemotron, >= 8-10 GB for Voxtral. AMD/Intel: Vulkan or DirectML path, treated as "needs benchmark".
   - Check free disk space against the model download size.
2. **Dynamic micro-benchmark (5-10 s, after model download or with a bundled tiny test clip)**
   - Feed a 10 s bundled speech clip (RU+EN mix) through the candidate engine in streaming mode and measure **real-time factor (RTF)** and **time-to-first-partial** and **final latency after end of speech**. Run it as two concurrent streams to simulate mic + system.
   - Pass criteria (suggested): RTF <= 0.5 with both streams, p95 finalization delay < 800 ms, no thermal throttling in a 20 s run (check a second 5 s pass).
   - Run candidates in order of preference; stop at the first pass. Cache the result keyed by (device, driver, app version); re-run on hardware/driver change or if runtime monitoring sees sustained RTF > 0.8 (auto-suggest cloud).
3. **Runtime guard**: monitor queue depth (audio waiting to be decoded). If backlog > 1.5 s for > 5 s, drop to a lighter engine/chunk size or prompt "Switch to cloud?" so the user is never silently > 1 s behind. Battery/low-power mode: warn.
4. UX: show the result as "Local: OK (Nemotron, ~0.4 s)" or "This device is too slow for local; use cloud" with a one-click choice.

---

## 6. Rust / Tauri integration

| Need | Crate / binding | Notes |
|---|---|---|
| Whisper | **`whisper-rs`** (whisper.cpp bindings). Features: `cuda`, `metal`, `vulkan`, `hipblas` (Linux), `openblas`; CoreML not listed in README. Repo archived on GitHub; **maintained on Codeberg** [P](https://github.com/tazz4843/whisper-rs) | Or call whisper.cpp directly via our own `cc`/`bindgen` build if CoreML needed. Models ggml: turbo q5_0 ~0.55 GB |
| Parakeet / Nemotron | **`parakeet-rs`** on ONNX Runtime; supports CTC and TDT models, streaming (ParakeetEOU and Nemotron), 16 kHz mono; execution providers CUDA, TensorRT, WebGPU, DirectML [P-ish: docs.rs](https://docs.rs/parakeet-rs). Small project (254 stars). Alternatively NeMo-Speech.cpp (C++, GGUF Q8_0, WebSocket/CLI) [S](https://wavect.io/blog/nvidia-nemotron-3-5-asr-production-review/) | Check that the multilingual Nemotron 3.5 export is supported before committing. |
| Generic ONNX | `ort` crate (ONNX Runtime) with CoreML EP on Mac, DirectML on Windows | Unified path for Silero VAD, Nemotron, Parakeet |
| sherpa-onnx | Streaming + non-streaming ASR, VAD; Rust bindings via `sherpa-rs` (separate project) [P](https://github.com/k2-fsa/sherpa-onnx) | Good if its model zoo adds Nemotron/Parakeet streaming |
| Multi-engine wrapper | `transcribe-rs` (README fetch failed; verify existence, engines supported) | Could save time; evaluate |
| Voxtral on Mac | MLX only: use the Swift port (`voxtral-mini.swift`) or a small Swift/Python sidecar; Rust port exists but unvetted [P](https://huggingface.co/mistralai/Voxtral-Mini-4B-Realtime-2602) | Native Swift module behind a Tauri plugin/sidecar |
| VAD | Silero VAD (ONNX, via `ort`) or whisper.cpp's built-in | |
| Mac native option | WhisperKit (Swift, MIT, macOS 14+) as a Tauri sidecar or plugin [P](https://github.com/argmaxinc/WhisperKit) | Real-time streaming with speakers is in the paid Pro SDK |

Architecture suggestion: a Rust `SttEngine` trait producing `Partial{stream, text, t0}` / `Final{...}` events; engines: `NemotronOnnx`, `WhisperCpp`, `VoxtralMlx` (Mac sidecar), `CloudWs{Soniox,Deepgram,...}`. Two instances per session (mic and system), isolated.

Model download sizes (approx.): Nemotron 3.5 0.6B 470-730 MB (MLX 4/5/8-bit; CoreML INT8 612 MB; ONNX INT8 ~650 MB); Whisper large-v3-turbo ggml q5_0 ~0.55 GB, q8_0 ~0.9 GB, fp16 ~1.6 GB; WhisperKit turbo compressed 626 MB; Voxtral 4B MLX 4-bit/6-bit/8-bit ~2.2-4.5 GB (my estimate from 4B params), bf16 ~8-9 GB; Parakeet v3 INT8 ~680 MB; Silero VAD ~2 MB. Download on demand, not bundled, to keep the app light.

---

## 7. Recommendation

1. **Mac default**: Nemotron 3.5 Multilingual (MLX 5-bit or CoreML INT8), chunk 320-560 ms -> end-to-end ~0.5-0.9 s. Two streams are cheap (<1.5 GB each peak). Upgrade path "High-quality": Voxtral Realtime 6-bit MLX on 16 GB+ Pro/Max chips (RU WER 6%, but 13 languages only).
2. **Windows + NVIDIA**: Nemotron 3.5 via ONNX Runtime CUDA (or NeMo-Speech.cpp); Whisper large-v3-turbo via whisper.cpp CUDA as the high-accuracy option (final-quality, ~1 s partial).
3. **Windows CPU/iGPU**: try Nemotron INT8 on CPU via micro-benchmark; if it fails -> cloud. Do not ship Whisper-turbo-on-CPU as a realtime option.
4. **Cloud default**: Soniox stt-rt-v5. Alternates: Deepgram Nova-3 Multilingual (documented RU/EN code-switching), Mistral Voxtral Realtime API (cheap, 13 languages), OpenAI gpt-live-transcribe.
5. **Whisper's role**: (a) selectable engine "Whisper turbo (accurate)" using the VAD + partial-redecode + LocalAgreement design in 1.4; (b) the *final-quality* pass: after a call ends (or per utterance in background) re-transcribe with Whisper turbo for the saved .md, which is also the best fit for the "huge volume of calls" context store, and the language fallback for languages Nemotron/Voxtral do not cover; (c) never the default realtime engine on low-end machines.
6. **Mixed language**: pick the engine per session by language set: Nemotron/Voxtral cover the common set; unusual languages go to Whisper or cloud.

## 8. Key risks
- **Code-switching quality is unproven for every local candidate** (Nemotron, Voxtral, Whisper). Only Deepgram documents RU+EN; Soniox and Gladia imply it. A bake-off on our own RU+EN corpus (finance/IT jargon like "EBITDA") is the #1 task.
- Nemotron 3.5 is new (mid-2026): runtime maturity in Rust/ONNX and the multilingual export, plus its HF license text, need verification; 8 of 40 locales not production-ready; Russian WER not published.
- Voxtral local runtimes outside Apple MLX are immature; vLLM-only officially. 4B model is borderline on base Macs.
- "< 1 s" is easily met by cloud and native streaming models, but not by Whisper except for finals on strong hardware; set UI expectations.
- Two simultaneous streams double compute and thermal load (laptops on battery).
- Cloud prices change and many are promotional (Deepgram streaming "limited-time"); the OpenAI price is third-party only; ElevenLabs availability by region unclear.
- Third-party audio goes to cloud vendors: legal/consent messaging.
- whisper-rs moved to Codeberg; bus-factor risk for bindings (`parakeet-rs` is small too).

## 9. Benchmarks to run before locking defaults
1. 30-min RU+EN code-switch test set (real meeting audio + "me" mic + "them" Zoom/Teams system audio). Metrics: WER, English-term accuracy, language-flapping rate.
2. On M1 8 GB, M2 16 GB, M3/M4 Pro: RTF and first-partial/final latency for Nemotron (MLX/CoreML), Whisper turbo q5_0 (+CoreML), Voxtral 6-bit. Both streams concurrently, 20 min sustained.
3. Windows: RTX 3060 (CUDA), AMD/Intel iGPU (Vulkan/DirectML), CPU-only i5/i7 and Ryzen 5: Nemotron ONNX INT8, Whisper turbo.
4. Cloud bake-off: Soniox vs Deepgram vs Gladia vs OpenAI vs Voxtral API on the same audio; measure time-to-first-token and final latency from a Moscow/EU/US client.
5. Verify: Nemotron HF card license + Russian WER; AssemblyAI streaming languages; OpenAI realtime transcription pricing; ElevenLabs availability and pricing; Moonshine language list; `transcribe-rs` suitability.

## 10. Sources
- Voxtral model card: https://huggingface.co/mistralai/Voxtral-Mini-4B-Realtime-2602 ; announcement/pricing: https://mistral.ai/news/voxtral-transcribe-2 ; API docs: https://docs.mistral.ai/studio/audio ; Swift port: https://github.com/mzbac/voxtral-mini.swift ; MLX community: https://huggingface.co/flo5k5/Voxtral-Mini-4B-Realtime-2602-MLX-6bit
- Nemotron: https://wavect.io/blog/nvidia-nemotron-3-5-asr-production-review/ ; https://soniqo.audio/guides/nemotron ; https://openwhispr.com/blog/parakeet-vs-whisper-vs-nemotron ; https://catalog.ngc.nvidia.com/orgs/nvidia/collections/nemotron-asr-streaming
- Whisper: https://github.com/ggml-org/whisper.cpp ; https://github.com/ufal/whisper_streaming ; https://github.com/argmaxinc/WhisperKit ; https://github.com/tazz4843/whisper-rs ; https://github.com/anvanvan/mac-whisper-speedtest ; https://www.promptquorum.com/local-llms/apple-silicon-whisper-metal-benchmark ; https://arxiv.org/pdf/2410.04527 ; https://friendli.ai/models/coriollon/whisper-large-v3-turbo-russian-codeswitch
- Other local: https://github.com/kyutai-labs/delayed-streams-modeling ; https://github.com/moonshine-ai/moonshine ; https://docs.rs/parakeet-rs ; https://github.com/k2-fsa/sherpa-onnx
- Cloud: https://openrouter.ai/docs/guides/overview/multimodal/stt ; https://soniox.com/pricing ; https://soniox.com/docs/stt/rt/real-time-transcription ; https://developers.deepgram.com/docs/models-languages-overview ; https://deepgram.com/pricing ; https://www.assemblyai.com/pricing.md ; https://www.speechmatics.com/pricing.md ; https://developers.openai.com/api/docs/guides/realtime-transcription ; https://costgoat.com/pricing/openai-transcription ; https://www.gladia.io/product/audio-intelligence/code-switching ; https://elevenlabs.io/realtime-speech-to-text
