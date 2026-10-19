# Lowk research: hotkey LLM (Topic A) and Jev slide detector (Topic B)

Date: 2026-10-19. Method: live OpenRouter catalog API (`/api/v1/models`, `/models/{id}/endpoints`), OpenRouter docs, Google/Gemma docs, typesafe.ai blog + docs (docs.typesafe.ai/llms.txt), web search.

**Confidence legend.** [V] = verified from a primary source today. [S] = secondary/third-party, inconsistent, treat as indicative. [U] = not measured / could not obtain. **I could not obtain real TTFT or tokens/sec numbers for the OpenRouter models**: the public API returns `latency_last_30m` / `throughput_last_30m` as null and the performance tab is JS-rendered. Section A1.4 gives a 30-minute harness to measure it before locking defaults. Do not treat any latency number for cloud models in this file as measured.

---

## TOPIC A — the answering LLM

### A0. Requirements recap (BRIEF §3)
Chat-style, delta-only requests (system prompt + growing history; each request adds only new transcript since the previous request), optional screenshots (vision required), optional retrieved docs, streamed output into overlay, TTFT is the key metric, BYOK in MVP, Russian/mixed-language speech must work.

### A1. Cloud via OpenRouter

#### A1.1 Current DeepSeek "Flash" lineup [V, OpenRouter catalog]
There are now several Flash variants; naming is confusing, so pin explicit slugs.

| Slug | Vision | In / out $/M | Cache read $/M | Notes |
|---|---|---|---|---|
| `deepseek/deepseek-v4.1-flash` | **yes (native)** | 0.032 / 0.08 (cheapest provider; most providers 0.14-0.30 / 0.42-1.2) | 0.0016 | V4.1 Flash, released 2026-09-10. Sparse MoE, 552B backbone, 8B active on input / 16B on output, new "CED" architecture, 1M ctx, compressed KV cache (~1/4 of previous Flash). Reasoning supported. 33 providers listed. |
| `deepseek/deepseek-v4.1-flash-1017` | yes | 0.15 / 0.60 | 0.003 | 2026-10-17 snapshot, served by DeepSeek first-party (`deepseek-flash` API name). |
| `deepseek/deepseek-v4-flash` ("0423") | no (text) | 0.091 / 0.182 | 0.018 | Older V4 Flash. |
| `deepseek/deepseek-v4-flash-0731` | no | 0.15 / 0.15 | 0.042 | |
| `deepseek/deepseek-v4-flash-vision-exp` | yes (experimental) | 0.216 / 0.647 | 0.007 | Vision add-on to 0731, released 2026-08-21. Superseded by 4.1. |
| `~deepseek/deepseek-v4-flash-latest` | no | alias | | Moving alias, avoid (behaviour drift). |

Recommendation: default `deepseek/deepseek-v4.1-flash` (text + screenshots in one model, so no model switching when a screenshot is attached). Note the first-party 1017 snapshot is the likely "reference quality" option; third-party fp4/fp8 hosts are cheaper but quantized.

**Reasoning on/off.** All Flash variants expose `reasoning` in `supported_parameters`. In the OpenRouter request use the unified `reasoning` object (`{"reasoning": {"enabled": false}}` or `{"effort": "low"}` / `max_tokens`), and `include_reasoning`/stream handling so reasoning tokens are not shown in the overlay unless desired. [Parameter shape: OpenRouter docs list `reasoning` and `reasoning_effort` as accepted; exact value shape must be checked in the API reference — U.] Expectation: reasoning-off gives first visible token after the prefill only; reasoning-on adds N hidden thinking tokens before the first visible token (hundreds of ms to many seconds), so **default OFF for the hotkey answer, offer "think harder" as a second hotkey/template**. Measured delta: [U], measure with harness.

#### A1.2 Fast alternatives (all via OpenRouter, all have vision) [V pricing/modalities from catalog]

| Model slug | Input modalities | In / out $/M | Cache read $/M | Ctx | RU / multilingual (prior, not measured) | Comment |
|---|---|---|---|---|---|---|
| `deepseek/deepseek-v4.1-flash` | text, image | 0.032-0.30 / 0.08-1.2 by provider | 0.0016 | 1M | good (Chinese-English strong, Russian decent) | Cheapest; provider spread is huge, so routing matters. |
| `anthropic/claude-haiku-5.5` | text, image, file | 0.10 / 0.50 (flat across Anthropic/Bedrock/Vertex/Azure) | 0.01 | 1M | strong | Same price on 5 first-party-class hosts, so low provider variance; `cache_control` needs only 512-token minimum. Very good default "quality fast" pick. |
| `google/gemini-3.8-flash` | text, image, video, file, audio | 0.75 / 3.75 (priority tier 1.35 / 6.75) | 0.075 | 1M | very strong multilingual | Fast, best multilingual/vision OCR prior; ~7x Haiku price. Native audio input (could be a future STT shortcut). |
| `google/gemini-3.5-flash-lite` | text, image, video, file, audio | 0.30 / 2.50 | 0.03 | 1M | strong | Cheaper Google tier. `gemini-3.1-flash-lite` is 0.25 / 1.50 (also fine). |
| `openai/gpt-6-luna` (and `-pro`) | text, image, file | 0.10 / 0.50 | 0.01 | 1.05M | strong | New small GPT-6 tier, same price as Haiku 5.5. `openai/gpt-5.6-luna` 0.20 / 1.20. `~openai/gpt-mini-latest` currently = gpt-5.4-mini at 0.75 / 4.50 (stale price/perf, prefer luna). |
| `x-ai/grok-4.7` / `grok-build-0.1` | text, image, file | grok-4.7 2.00 / 6.00; grok-build-0.1 1.00 / 2.00 | 0.5 / 0.2 | 500k / 256k | OK | Not cheap; "fast" only in name now. Low priority. |
| `qwen/qwen3.8-flash` (also `qwen3.8-omni-flash`) | text, image, video (omni adds audio) | 0.15 / 0.47 | 0.016 | 1M | good (119+ langs claimed by family) | Single provider (Alibaba), so no provider failover; region latency from RU/EU/US varies. |
| `z-ai/glm-5.3-flash` | text, image, video | 0.048 / 0.16 | 0.0096 | 1M | weaker RU prior | Very cheap alternative. |
| `google/gemma-4-26b-a4b-it` | text, image, video | 0.04-0.15 / 0.18-0.60 | 0.05 | 262k | decent (140+ langs) | The cloud twin of the local model; also a `:free` variant (rate limited, no SLA). Useful because cloud/local answer styles match. |

Tokens/sec and TTFT [U]. Priors only (from architecture class, not measurement): small dense/MoE frontier "flash/haiku/luna" tiers typically stream 100-250+ tok/s with TTFT 0.3-1.0 s for short prompts; DeepSeek on third-party hosts varies widely by host and quantization; DeepSeek's V4.1 8B-active-on-input design should give fast prefill on large prompts (relevant for screenshot + long history).

Shortlist for the MVP model picker (BYOK users choose):
1. Default "fast+cheap, vision": `deepseek/deepseek-v4.1-flash`, reasoning off.
2. "Reliable fast": `anthropic/claude-haiku-5.5` (flat pricing, no provider lottery, strong Russian).
3. "Best multilingual/vision": `google/gemini-3.8-flash`.
4. "Cheap OpenAI-family": `openai/gpt-6-luna`.
5. Optional: `qwen/qwen3.8-flash`, `z-ai/glm-5.3-flash`.
Treat this list as data, not hard-coded: fetch `/api/v1/models` at runtime, filter `input_modalities` contains image and `supported_parameters`, and let the user favourite. Model IDs churn monthly (three DeepSeek Flash snapshots in 10 weeks).

#### A1.3 Provider routing for speed [V, OpenRouter docs]
- `provider.sort: "latency"` or `"throughput"` (string, or object `{by, partition}`); **setting sort disables load balancing**.
- Slug shortcuts: `:nitro` = sort by throughput (and makes priority-tier endpoints eligible); `:floor` = sort by price (flex tier eligible).
- Soft thresholds: `provider.preferred_max_latency` (seconds, or `{p50,p75,p90,p99}`) and `preferred_min_throughput` (tok/s, same shape). Misses are deprioritized, not excluded.
- `allow_fallbacks` (default true), `data_collection: "deny"`, `zdr: true` for privacy-sensitive meetings. Expose "privacy mode" = `data_collection:"deny"` + `zdr:true` (this shrinks the provider pool and may raise latency; warn the user).
- Priority/fast tiers exist: Google AI Studio `/priority` (1.8x price), OpenAI `/fast` (2x), BaseTen `/fast` for DeepSeek (2x). These are the real "fast lane"; `:nitro` makes them eligible.
- **Interaction with caching:** OpenRouter sticky-routes to the provider that served the last cached request (10-minute session TTL, keyed by hash of first system + first user message, or explicit `session_id` / `x-session-id`). Forcing `sort` or `:nitro` each call may bounce between providers and kill cache hits. Recommendation: on a meeting start, pick the provider once (probe), then pin with `provider.order:[X]` + `allow_fallbacks:true` and send a stable `session_id` (meeting id).
- Recommended request defaults: stream:true; `provider:{sort:"latency", preferred_max_latency:{p90:2}, ...}` for first request, then pin.

#### A1.4 Benchmark harness to run before shipping (30 min of work)
Script: for each candidate model x {text-only 1.5k tokens, +1 screenshot 1280x720, +10k history cached} x {reasoning off/on}, 20 runs each, stream, record t_first_content_delta (excluding reasoning deltas), tokens/sec over content, total, and OpenRouter `provider` returned in the header/`generation` endpoint (`/api/v1/generation?id=`) for per-provider attribution. Run from user-like regions (EU, US, RU via VPN) because the target audience is partly Russian-speaking. Also include a Russian prompt set (mixed speech, "Что значит EBITDA?") for quality and token-count: Cyrillic tokenizes 1.5-2.5x worse than English on most tokenizers, which affects both cost and TTFT.

#### A1.5 Latency-engineering notes (provider-independent)
- Hotkey press must open the HTTPS/2 connection in advance (keep-alive, DNS+TLS warmed when meeting starts; a 1-token no-op "warm" request every ~4 min can also keep provider cache alive).
- Pre-screenshot encoding: downscale to ~1280 px long edge JPEG q80 before sending; vision tokens dominate prefill. Offer "OCR-first"? No; keep it simple.
- Don't use structured output / tool calling in the hot path.
- Strip reasoning from display; for models that always reason, show an "thinking…" indicator based on reasoning deltas to avoid a dead overlay.
- Cap `max_tokens` (e.g. 400) and instruct brevity; this also bounds cost.

### A2. Local models and runtimes

#### A2.1 Gemma 4 [V Google docs ai.google.dev/gemma/docs/core; license [S]]
- Sizes: **E2B, E4B** (edge; audio-capable), **12B** (dense/unified; audio-capable), **26B A4B** (MoE, 25.2B total, ~3.8B active), **31B** dense. Secondary sources list only four (E2B/E4B/26B/31B) and disagree on the 12B; Google's own docs list five. [S/V conflict: check the model card before hard-coding.]
- Vision: text + image on **all** sizes, video supported, native audio on E2B/E4B/12B. Variable aspect ratio/resolution. Context 128k (small) / 256k (medium+).
- Weights-only memory (BF16 / 8-bit / 4-bit), Google: E2B 11.4/5.7/2.9 GB; E4B 17.9/8.9/4.5; 12B 26.7/13.4/6.7; 26B A4B 57.7/28.8/14.4; 31B 69.9/34.9/17.5. MoE must hold all 26B in memory. Add KV cache + vision encoder.
- License: reported moved to **Apache 2.0** (secondary sources; Google doc page says "responsible commercial use" with a prohibited-use policy and is flagged deprecated in favour of gemma.ai). AGPL project compatibility: Apache-2.0 weights are fine; verify on the HF model card before bundling/redistributing. [S]
- Quality: benchmark claims roughly tied with Qwen 3.5 class peers; 31B MMMU-Pro 76.9%, 140+ languages [S]. Russian: expected good for 12B+, usable but noticeably weaker/hallucination-prone for E2B/E4B (prior, not measured). Gemma 4 has a "thinking" mode and per-model draft models for speculative decoding [V].
- Mapping for Lowk's hardware tiers:
  - 8 GB Mac / weak PC: no local LLM, or E2B Q4 (~3 GB) for very short answers only.
  - 16 GB Mac / 8 GB GPU: **E4B Q4 (~4.5 GB)** or 12B Q4 (6.7 GB, tight with the STT model + overlay + the call app).
  - 32 GB+ Mac / 16-24 GB GPU: **26B A4B Q4 (~14.4 GB)**; best quality/speed trade because only ~4B active.
  - 64 GB+: 31B Q4.
  Remember Whisper/Voxtral (local STT) and the meeting app already compete for the same unified memory; budget accordingly.

#### A2.2 Alternatives to consider (not verified today; quickly check HF before including)
Qwen3.8/3.6 small VL models (`qwen3.6-35b-a3b`, `qwen3.5-9b` are in OpenRouter's catalog, so they exist as open-weight; vision, strong multilingual), Mistral Small (2603, 24B, vision), GLM-5.3-flash-class, Nemotron 3 Nano Omni 30B-A3B (free on OpenRouter; text/audio/image/video). Recommended: keep the local layer model-agnostic (any GGUF/MLX vision model the runtime can serve), with Gemma 4 as the curated default.

#### A2.3 Realistic speeds [S; mixed hardware/quant/context, not directly comparable]
- E4B, M4 Pro: llama.cpp Metal Q8_0 ≈ 39 tok/s (32 tok/s at 8k ctx); Ollama ≈ 59 tok/s reported; LiteRT-LM with MTP ≈ 100 tok/s. E4B on M5 Pro (SiliconBench, c=1): llama.cpp 18.3 tok/s with TTFT p50 1208 ms, mlx_lm 17.5 tok/s with 826 ms (suspiciously low vs other sources; different quant/prompt).
- 26B A4B: M4 Pro ≈ 53 tok/s (Ollama); M3 Ultra: Rapid-MLX 85 tok/s / 0.26 s TTFT, mlx-vlm 84 / 0.31 s, Ollama 75 / 0.08 s; M1 Max llama.cpp ≈ 40 tok/s gen / 395 tok/s prompt processing (so a 5k-token prompt costs ~12 s of prefill on that chip).
- Takeaways: (1) generation speed is fine on any M-series for E4B/26B-A4B (30-85 tok/s); (2) **prefill is the real TTFT cost** — prompt-processing speed 150-400 tok/s on base/Pro chips means a 3k-token first request is 8-20 s of TTFT unless the KV cache is reused; (3) screenshots add roughly 256-1k+ image tokens each; (4) local viable only with KV-cache reuse (the delta design helps a lot, see A3). Windows: NVIDIA RTX 30/40-class with ≥8 GB runs E4B/12B Q4 at 60-150+ tok/s with fast prefill (>1000 tok/s) via CUDA llama.cpp [prior, not measured]; iGPU/CPU-only: E2B/E4B only, ~5-15 tok/s and slow prefill — recommend cloud.
- Treat all as "needs measurement on our reference machines (M2 8/16 GB, M4 Pro, RTX 3060/4070, Ryzen iGPU)". Add a first-run 10-second hardware self-benchmark (also feeds BRIEF §1 "app decides whether hardware can handle local model").

#### A2.4 Runtime choices
| Option | Pros | Cons |
|---|---|---|
| Bundle llama.cpp (`llama-server`, GGUF) | One engine for Mac (Metal) + Windows (CUDA/Vulkan/CPU); OpenAI-compatible API; vision via mtmd; prompt cache + `cache_prompt` slot reuse; speculative decoding; MIT license; small binary (~10-30 MB per backend) | We own updates/backends (CUDA vs Vulkan vs CPU variants -> installer size); GGUF vision support lags new architectures by days-weeks |
| Bundle MLX (mlx-lm / mlx-vlm) | Often fastest on Apple Silicon for MoE; | Mac-only; Python runtime or Swift port; heavy to embed; contradicts "lightweight" |
| Connect to Ollama | Users already have it; huge model library; llama.cpp backend, TTFT best in the M3 Ultra table | Extra install, background daemon; model auto-unload/keep_alive default 5 min (set `keep_alive:-1`); API `/api/chat` + OpenAI-compatible `/v1` |
| Connect to LM Studio | Nice UX, MLX + llama.cpp backends, OpenAI-compatible server at `localhost:1234`, headless CLI (`lms`) | Closed-source app; less common |

**Recommendation.** MVP: do not bundle. Implement a single "OpenAI-compatible endpoint" provider with presets for Ollama (`http://localhost:11434/v1`), LM Studio (`:1234/v1`), and custom `llama-server`; auto-detect running servers on start, list models, flag vision-capable ones, offer a one-click "install Ollama" link. This keeps the app lightweight (BRIEF requirement) and BYOK-simple. Phase 2: optional downloadable sidecar `llama-server` (per-platform, downloaded on demand, not in the installer) managed by Tauri as a sidecar, with Gemma 4 E4B / 26B-A4B Q4 GGUFs curated by the hardware self-benchmark. Skip MLX bundling unless benchmarks on 16 GB Macs show a >1.5x TTFT gain; it can arrive via LM Studio users for free.

### A3. Prompt caching and the "delta only" design

**The chat-shaped design is exactly what prefix caching rewards**, but "only send delta" must be interpreted correctly: the API is stateless, so the client still has to resend the full history on every call (system prompt + all previous user deltas + all previous assistant answers) as an append-only prefix. The "delta" is only the *new tail*. Properly done, every request after the first has a cache-hit prefix and only the new delta pays full price/prefill.

Rules for Lowk:
1. **Strictly append-only message list per meeting.** Never reorder, edit, re-timestamp, or re-render earlier turns (a single byte change invalidates everything after it). Store the exact serialized messages; don't regenerate from structured data.
2. Stable prefix order: system prompt (template, language, persona) -> pinned meeting context/retrieved docs (inserted once at meeting start or when the user attaches docs; later retrieval results go in the *new* user turn, not mutated earlier ones) -> chat turns.
3. Put volatile things (current time, "now" screenshot, new retrieved snippets) only in the latest user message. No timestamps in the system prompt.
4. Screenshots: images stay in history after use only if you want the model to remember them; each costs tokens on every subsequent call (cache-read priced though). Option: replace old images with a text caption after the answer — but that edits the prefix, so do this only at "compaction" points (below).
5. **Compaction** when the history exceeds budget (e.g. 30-50k tokens): summarize the oldest part into a new prefix once, accept one full cache miss, then continue append-only. Long multi-hour calls will need this; 1M context windows delay it but not cost.
6. Dead time kills caches: provider TTLs are short. Anthropic default 5 min (1 h with `ttl:"1h"`, write 2x), Gemini implicit ~3-5 min, OpenAI explicit min 30 min on GPT-5.6+ (automatic caching otherwise opportunistic). Hotkey usage is sporadic (minutes between presses), so choose by pricing: with DeepSeek/Grok/Gemini/OpenAI automatic caching there is no write premium, so misses just cost normal input price — cheap anyway. For Claude, use `cache_control` with 1 h TTL for the system+context prefix and 5 min for the rolling tail; min cacheable prompt is only 512 tokens for Haiku 5.5.
7. Per-provider behaviour [V OpenRouter docs]: DeepSeek automatic, read 0.1x, write 1x; OpenAI automatic >=1024 tokens (GPT-5.6+: write 1.25x, explicit breakpoints); Anthropic needs `cache_control` (<=4 breakpoints; write 1.25x/2x, read 0.1x); Gemini implicit read 0.25x, explicit fixed 5 min TTL and its system instruction is immutable when cached; Grok automatic, read 0.25x.
8. **Provider stickiness** is required for hits: send a stable `session_id` per meeting; avoid per-request `sort`/`:nitro` re-ranking (see A1.3); `provider.order` pin overrides stickiness safely.
9. Numbers to expect: with DeepSeek V4.1 Flash cache read at $0.0016/M vs $0.032+/M input, a 3-hour meeting with 40k-token context and 30 hotkeys costs well under $0.05 on input. Cost is not the issue; **TTFT is** — cached prefix skips prefill, which matters most for local models and for screenshot-heavy contexts.
10. Local runtimes: llama.cpp `cache_prompt:true` + a fixed slot (`id_slot`) reuses the KV cache for the longest common prefix, giving the same append-only benefit; Ollama keeps the context across calls if the prefix matches and the model stays loaded (`keep_alive:-1`; set `num_ctx` explicitly, it silently truncates otherwise). With these, a delta request prefills only the new tokens, which makes E4B/26B-A4B responsive even on 16 GB Macs.
11. Failure modes: model switch mid-meeting = full miss; changing the template mid-meeting rewrites the system prompt = full miss (instead append a user-level instruction); changing reasoning on/off may also split the cache on some providers.
12. The user can "reset context" -> new session_id and fresh prefix.

### A4. Recommendations (Topic A)
1. Provider abstraction: `OpenAI-compatible chat/completions streaming` for OpenRouter, Ollama, LM Studio, llama-server. One code path.
2. Default cloud: `deepseek/deepseek-v4.1-flash`, reasoning off, `session_id`=meeting, provider pinned after a first latency probe, `data_collection:"deny"` toggle. Offer `claude-haiku-5.5` as the reliable alternative and `gemini-3.8-flash` for Russian/OCR-critical use.
3. Local: Ollama/LM Studio connectors first; Gemma 4 E4B (16 GB) / 26B-A4B (32 GB+) curated; hardware self-benchmark gates the recommendation.
4. Run the A1.4 harness before fixing defaults; the numbers in this report for latency are priors.
5. Message-store design: append-only serialized transcript per meeting, compaction as an explicit event.

---

## TOPIC B — Jev (typesafe.ai) and the slide-change detector

### B1. What Jev is [V: typesafe.ai blog posts + docs.typesafe.ai]
- First public **"System One" model** from TypeSafe AI (post dated 2026-09-15 per header / 2026-10-15 per metadata; early access via waitlist; **Jev 1.14 available from 2026-10-14**). A "frontier-intelligence function call": takes unstructured state, returns **typed, calibrated structured decisions**, not free text. Trained with "Reinforcement Learning for Calibrated Decisions (RLCD)", new architecture with **parallel sampling** (all outputs in one pass, not token by token).
- Questions (primitives): **Choice** (pick one of up to 255 options; probabilities + confidence), **Score** (ordered levels; probabilities + confidence), **Noul** (yes/no; returns probability 0-1 of yes), **Box** (bounding box, Jev 1.14 only, fractions of image width/height). Questions run in parallel and in isolation on the same state; adding questions barely changes latency (cookbook: 13 questions batched = 10x faster, 12x cheaper than separate calls).
- Weaknesses it documents (jev-1.13 jaggedness): literal reading, poor counting, weak on dates/numeric representations, multi-hop reasoning, "context rot" on large/irrelevant state, prompt-injection sensitivity, favours first Choice option, cannot generate text. Page does not discuss OCR or multilingual (1.14 claims improved multilingual).

### B2. Image input [V docs.typesafe.ai/images]
- Supported in `jev-1.13.0` and `jev-1.14.0`, but gated: header `TypeSafe-Danger-Zone-Images: true`. Officially **"not yet up to production standard"**; format/limits may change between releases; the 1.14 post says multimodal quality is **worse than text-only**, recommends simple decomposed questions.
- Formats: PNG or JPEG (still), upright pixels (EXIF stripped, not applied). **Each side 280-1568 px; total pixels 78,400-2,458,624; 16 MiB per image; request body 32 MiB.** Out-of-range images are **rejected (400/413), not resized**: client must resize (e.g. fit to 1568 long side, ensure >=280 short side; 2.46 MP cap means e.g. 1920x1080 = 2.07 MP is OK, 2560x1440 = 3.7 MP is not).
- Sending: top-level `assets` map `{ref: {type:"image", format:"jpg"|"png", data_base64}}` (or multipart file, recommended; base64 +33%); put the ref string in `state`, `instructions` or `criteria`; every asset must be referenced. Each reference use duplicates the image (images in per-question `instructions` are sent once per question: put shared images in `state`).
- Billing: image tokens counted as normal input tokens (count towards context); token-per-image guidance "coming" [U].

### B3. API shape [V]
- `POST https://api.typesafe.ai/v1/systemone`, `Authorization: Bearer <key>` (keys at console.typesafe.ai). Models: `jev-1.13.0`, `jev-1.14.0`; aliases `jev-latest` (-> 1.13.0, moves to 1.14.0 "in about a week") and `jev-preview` (-> 1.14.0). Pin versioned IDs for stable behaviour; deprecation notice 60 days (1.13) / 30 days (1.14).
- Request: `{ "model", "state": string|object, "assets"?, "questions": { id: { "type":"noul", "instructions": "...", "criteria": {"true": "...", "false": "..."} } } }`.
- Response: `{ "model": "jev-1.13.0", "answers": { id: {"type":"noul","noul":0.93} }, "usage": {"input_tokens", "output_tokens"} }`. No timing fields in the response. Python and JS SDKs exist (they handle asset upload); curl works.
- Context: 64k per request / 32k per question by default; `TypeSafe-Danger-Zone-Long-Context: true` raises both to 262k (slower, less accurate).
- Rate limits per organization (tokens and requests per second), visible in the console. Free output.

### B4. Latency, price, local vs cloud, license
- **Latency:** claimed end-to-end **70-500 ms**, "40x-200x faster than frontier LLMs on System One-shaped queries"; benchmarks run from West Coast US laptops, service located in the US West [V blog, self-reported]. No latency numbers in docs. With images and from Europe/RU expect more; measure. Long context slower.
- **Price:** **$0.042 per million input tokens, output free** (both 1.13 and 1.14). Blog admits possible subsidy. A ~1.5-2.4 MP image at roughly 1-3k tokens [guess, U] is therefore ≈ $0.0001 per check: 12 checks/min for 3 h ≈ 2,160 calls ≈ $0.2-0.3. Negligible.
- **Local vs cloud:** **cloud API only** found; no weights, no self-hosting or on-prem mentioned anywhere (legal page lists only DPA, MCA, Privacy Policy). Enterprise zero data retention by contacting sales; company says it does not train on customer queries (opt-in sharing exists).
- **License:** no model license; it's a proprietary hosted service under the Master Customer Agreement. Only the SDKs/adapters are open (github.com/typesafe-ai/system-one-adapter-python). **Implication for Lowk (AGPL, BYOK):** Jev must be an optional plugin where the user supplies their own TypeSafe key; no bundled key. Slide images leave the device (meeting screen content) — requires explicit opt-in and a privacy note alongside the existing recording-legality warning. Status: early access/waitlist, "danger zone" image feature, 1.14 weeks old, vendor risk is high -> **design the detector so that Jev is replaceable** by (a) a cheap vision LLM yes/no (e.g. Gemini 3.8 Flash / Haiku 5.5 / DeepSeek V4.1 Flash with `max_tokens:1`) or (b) no confirmation at all (high-precision local rules only).

### B5. Slide-change detector design

#### B5.1 Requirements and the hard part
Screen share of a presentation/video call: need to save a snapshot when a *new slide* appears and also when a slide *gains new important content* (bullet appears, animation build, a figure revealed, code line added) even when 90%+ of pixels are unchanged. Must avoid triggering on: webcam tiles moving, cursor/laser pointer, presenter's face video, notification toasts, active-speaker borders, compression noise/keyframe pulses, scroll/zoom of shared doc, subtle transitions/fades, "recording" indicators, clock widgets. Cost/battery: run at low rate on a downscaled capture of the shared region only.

Core insight: global similarity (SSIM, pHash of whole frame) is exactly the wrong statistic: a new bullet changes <5% of pixels. Use **localized, structure-preserving change detection** and ask "is there a *stable, text-like, localized* change", not "is the frame different".

#### B5.2 Capture
- Capture the shared-content region (user selects the window/screen or auto-detect biggest non-webcam rect) at **1 fps** (0.5 fps when idle for >30 s; 2-4 fps burst for 3 s after a detected change to catch builds/animations). Capture via ScreenCaptureKit (macOS) / Windows Graphics Capture; request low-res stream where possible (cheap), full-res only when saving.
- Work on grayscale, resized to a fixed analysis size, e.g. **384x216** (or 480x270 for 4k text-heavy slides) with area-averaging (INTER_AREA) — keeps thin text as gray smudge instead of aliasing. Keep a second 1280-wide copy for saving/Jev only when triggered.

#### B5.3 Stage 1: local pre-filter (cheap, recall-first)
Per frame t compared with the **last accepted snapshot S** (not the previous frame — avoids slow-fade drift missing) and also with t-1 (for stability):
1. **Block diff grid:** split into 16x9 (or 24x14) cells. For each cell compute mean absolute difference (MAD) between blurred (Gaussian σ≈1) gray frames, and a per-cell SSIM-lite or normalized cross-correlation. A cell is "changed" if MAD > T_cell (≈ 6/255 after blur; tune for noise floor measured from static periods, e.g. mean + 4σ of per-cell noise over the last 10 stable frames).
2. **Edge/text-aware weighting:** compute an edge map (Sobel/Canny) per cell; new text shows up as a rise in edge density/ink in a cell that was previously flat background, or a change in the edge pattern. Feature per cell: Δ edge density and Hamming distance of a 8x8 per-cell dHash. Changes in textured areas that are video-like (high temporal noise cell) are down-weighted by the cell's own noise history (maintain per-cell running variance; cells with high variance for many seconds = "video tiles / animations" and are masked from decisions).
3. **Aggregate with connected components** over changed cells: let A = total changed area fraction, C = largest connected component (area, bbox), and "added ink" = positive Δ edge density sum.
   - **Hard new-slide trigger:** A >= 25-30% of the content area, or global pHash Hamming distance >= ~12/64 on the 32x32 DCT pHash, or histogram (background-color) change.
   - **Small-addition trigger (the critical path):** any single connected changed region with **area >= ~0.4% of frame (≈ 1 line of text at normal slide scale; start with >= 2 adjacent cells of the 16x9 grid or one 24x14 cell) AND positive added ink (edge density increases) AND stable for >= 2 consecutive frames (≥ 1-2 s)** is a *candidate*. No upper bound — this deliberately fires on 90-99% similar frames. The thin additions that must not be lost are text lines/bullets; with a 384x216 analysis size a 24 px-high text line on 1080p becomes ~5 px: still visible as an edge-density change. Raise to 480x270 if slides use tiny fonts.
   - **Reject-as-noise:** isolated single cell with Δ below 2x noise floor; region touching the presenter/webcam mask; cursor-sized blob (≤ 1.5% of one cell dimension, moving between frames); region whose bbox moves frame-to-frame (cursor, pointer, drag); changes confined to screen edges (toasts, "you are sharing" banners) — keep a learned/manual mask of ignore zones.
4. **Debounce / stability:** wait until the frame stops changing (N=2 consecutive frames with A_t,t-1 < noise) before emitting, to capture the settled slide after a transition/animation. Cap the wait at ~4 s to avoid waiting for continuous video.
5. **Quiet-window re-check:** keep the strongest candidate in a ring buffer; after stability, compare final vs S once more (so a bullet that appears and is covered in the same second still counts).
6. **Never-miss safety net:** every K=20-30 s unconditional low-priority compare of current vs S using a *more sensitive* threshold (half the small-addition thresholds, no stability needed) — if it exceeds that, send to Jev anyway. Plus a manual snapshot hotkey (BRIEF §6) and "periodic snapshot" (e.g. every 2 min) as last resort.

Suggested starting thresholds (to tune on a recorded corpus; these are priors):
| Signal | Setting |
|---|---|
| Analysis frame | 384x216 gray, Gaussian σ=1, 1 fps (burst 3 fps) |
| Grid | 24x14 cells (≈16 px each) |
| Cell changed | MAD > max(5/255, 4σ noise floor) |
| Candidate region | ≥ 2 connected changed cells (~0.6% area) and added ink > 0 |
| Stability | 2 consecutive settled frames |
| New-slide (skip nuance) | changed area ≥ 25% or pHash(32x32) Hamming ≥ 12/64 |
| Cooldown | 1.5 s after emission; min 3 s between saved snapshots except for "new slide" class |
| Safety net | 20-30 s sensitive recheck |
Target operating point: recall ≥ 99% on "meaningful content additions" at the pre-filter (false positives are cheap because Jev costs ~$0.0001 and 100-500 ms), precision of 20-40% after stage 1 is acceptable; Jev lifts precision.
Implementation sketch (not app code): Rust with `fast_image_resize` + hand-rolled block diff, or OpenCV-lite; per-frame cost at 384x216 is sub-millisecond to ~2 ms; no GPU needed. Avoid heavy SSIM on full frame; SSIM per cell on 24x14 grid is fine if wanted. Perceptual hash alone is unsuitable as the *main* test because it is designed for global similarity robust to small edits — exactly the edits we want to catch; use it only for the global "obvious new slide" shortcut and for dedup of stored slides.

#### B5.4 Stage 2: Jev confirmation
Send the previous accepted snapshot S and the candidate frame N (both resized into Jev's limits: ≤1568 px side, ≥280 px, ≤2.45 MP; e.g. 1280x720 JPEG q85 = ~0.92 MP each) in `state` (shared images once), with the changed-region bbox crop optionally as a third image (zoom on the diff = much better for small text). Header `TypeSafe-Danger-Zone-Images: true`; model pinned `jev-1.14.0` (re-evaluate vs 1.13; 1.14 claims better multilingual).
One request, several Noul questions (fan-out is free-ish):
- `new_slide`: "Is image `new` a different slide (different title/topic/layout) than image `old`?"
- `content_added`: "Does image `new` contain substantial new text, bullets, figure, number or code that image `old` lacks, beyond cursor, webcam video or UI chrome?"
- `only_chrome`: "Are the differences only a mouse cursor, a video participant, a notification, a loading/progress indicator or compression noise?"
- `has_readable_text` (to skip blank/transition frames), `is_mid_transition`.
Decision in code (Jev gives probabilities and confidence, use them): save if `new_slide > 0.5` OR (`content_added > 0.6` AND `only_chrome < 0.5`); if confidence low or both mid-range (0.3-0.7) -> **fail open: save** (cost of an extra saved slide is tiny; missing a slide is the failure to avoid); if `is_mid_transition > 0.6` -> wait 1 s and re-evaluate. Caveats from the docs: counting/numeric/dates weak (don't ask it to count bullets); Choice favours first option (prefer independent Noul questions); images are officially experimental and "worse than text" -> keep questions simple and short; consider placing the crop of the changed region in the instructions as well. Dedupe: also apply a final local pHash/text-hash dedup against stored slides to avoid duplicates.
Fallback chain: Jev error/429/timeout (>1.5 s) -> either fail-open (save and mark unverified) or a vision LLM yes/no via the user's OpenRouter key (e.g. `deepseek/deepseek-v4.1-flash`, `max_tokens:3`) -> or accept stage-1 result directly. Setting: "Slide detection: local only / local + Jev / local + vision LLM".
Evaluation plan: record ~10 real meetings (Zoom/Meet/Teams screen shares: slide decks with builds, code walkthroughs, docs scrolling, Figma, video) with manual labels of "worth saving" moments (start with synthetic: take a deck, generate frames with 1 bullet added, 1 word added, number changed, image revealed, cursor moves, webcam noise), measure recall/precision at stage 1 and stage 2, tune thresholds, and compare Jev vs 2-3 vision LLMs on the same candidates (accuracy, p50/p95 latency from user regions, cost). Add OCR-free "text changed" fallbacks later (Apple Vision / Windows OCR are free locally) as an alternative stage 2: OCR both frames at the changed bbox and diff the strings, which handles "small text added" with no cloud at all and is a strong local option for privacy-focused users. Recommended as the default local confirmation if Jev proves immature.
Privacy: snapshots from screens may contain third-party data; Jev/LLM confirmation sends them to a cloud service -> opt-in, warn, and prefer sending only the cropped diff region when possible; local-only mode via OCR diff.

### B6. Recommendations (Topic B)
1. Treat Jev as an optional, replaceable stage-2 verifier behind a trait/interface; do not make it a hard dependency (proprietary, cloud only, early access, image support in "danger zone", no license for local use).
2. Stage 1 must be region/ink-aware and recall-first as designed above, global SSIM/pHash only as an "obvious new slide" shortcut.
3. Fail open on uncertainty; use Jev probabilities+confidence; use fan-out Noul questions; pin `jev-1.14.0`.
4. Build the labelled test corpus first; thresholds above are starting priors.
5. Ask TypeSafe (sales@typesafe.ai / console) about: latency from EU, SLA, image token cost, on-prem/weights availability, a license compatible with an AGPL open-source client, and rate limits for a BYOK model.

---

## Sources
- OpenRouter live catalog: https://openrouter.ai/api/v1/models and `/api/v1/models/{id}/endpoints` (queried 2026-10-19; prices, modalities, providers)
- OpenRouter DeepSeek V4.1 Flash: https://openrouter.ai/deepseek/deepseek-v4.1-flash ; V4 Flash: https://openrouter.ai/deepseek/deepseek-v4-flash ; Vision Exp: https://openrouter.ai/deepseek/deepseek-v4-flash-vision-exp
- OpenRouter prompt caching: https://openrouter.ai/docs/guides/best-practices/prompt-caching
- OpenRouter provider routing: https://openrouter.ai/docs/guides/routing/provider-selection
- Gemma 4 docs: https://ai.google.dev/gemma/docs/core ; license/coverage: https://www.noze.it/en/insights/gemma-new-release/ , https://infoq.com/news/2026/04/google-gemm4
- Local benchmarks: https://github.com/iprajax/gemma4-mtp ; https://arxiv.org/pdf/2609.19169 (SiliconBench) ; https://arxiv.org/pdf/2607.00501 (BaseRT) ; https://sudoall.com/gemma-4-31b-apple-silicon-local-guide/ ; https://g13n.substack.com/p/gemma-4-is-here-i-tested-all-four
- Jev: https://typesafe.ai/blog/introducing-system-one-models-and-jev ; https://typesafe.ai/blog/jev-1-14 ; https://docs.typesafe.ai/introduction ; /models ; /images ; /primitives/noul ; /model-jaggedness/jev-1.13 ; /legal ; index https://docs.typesafe.ai/llms.txt ; console https://console.typesafe.ai ; evals https://evals.typesafe.ai
