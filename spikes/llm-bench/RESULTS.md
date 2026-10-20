# LLM latency via OpenRouter (2026-10-20)

Script: `bench.py` (streaming SSE, httpx; key from env `OPENROUTER_API_KEY` only), `report.py` (table). Raw per-run data: `results.json` (no keys).

**Setup.** System prompt (RU, "answer in the question's language, max 2-3 sentences") + ~9.5k-char RU meeting transcript delta with English terms (~3.0-4.5k prompt tokens depending on tokenizer) + question «А что значит EBITDA?», with or without one 1280x720 PNG slide (Q3 financials). 8 runs per cell, rounds interleaved across models, unique nonce per request so prefix caches do not help (worst-case first ask), `max_tokens` 400, `reasoning.enabled=false`. TTFT = first non-empty **content** delta from request start (single client, Mac in one region, one afternoon, so p95 on n=8 is basically the max). tok/s = completion tokens / content streaming time. Cost = `usage.cost` reported by OpenRouter x100, uncached. Slugs verified in `/api/v1/models`; all six exist (also `deepseek/deepseek-v4.1-flash-1017`, added as a bonus).

**Reasoning cannot be disabled** on `google/gemini-3.8-flash` and `z-ai/glm-5.3-flash` (HTTP 400 "Reasoning is mandatory for this endpoint"); they were run with `reasoning.effort=low` and `max_tokens` 1500, so their TTFT includes thinking.

| model | variant | TTFT p50 / p95 (s) | tok/s p50 | $/100 asks | notes |
|---|---|---|---|---|---|
| deepseek-v4.1-flash (default routing) | text | 2.30 / 6.03 | 139 | 0.057 | provider varies each call (6+ hosts) -> high jitter |
| ... same | text+image | 2.80 / 12.52 | 127 | 0.065 | |
| deepseek-v4.1-flash `provider.sort=latency` | text | 1.89 / 2.56 | 303 | 0.106 | mostly Together; cost ~2x |
| ... same | text+image | 1.81 / 3.89 | 199 | 0.122 | |
| deepseek-v4.1-flash:nitro | text | 1.39 / 2.39 | 380 | 0.108 | Together/BaseTen; cost ~2x |
| ... same | text+image | 1.78 / 5.84 | 402 | 0.127 | |
| **deepseek-v4.1-flash-1017** | text | **1.18 / 1.70** | 294 | 0.059 | most stable DeepSeek option |
| ... same | text+image | **1.38 / 1.52** | 293 | 0.035 | |
| claude-haiku-5.5 | text | 1.46 / 1.65 | 249 | 0.052 | single provider, very stable; wordier (44 words) |
| ... same | text+image | 2.22 / 2.50 | 229 | 0.062 | |
| gemini-3.8-flash (reasoning low, forced) | text | 2.55 / 3.40 | 142 | 0.263 | 4-6x pricier (thinking tokens) |
| ... same | text+image | 5.09 / 14.72 | 146 | 0.350 | slow with image |
| gpt-6-luna | text | 1.65 / 1.91 | 135 | 0.040 | shortest answers (23 words), cheapest |
| ... same | text+image | 2.69 / 3.88 | 154 | 0.054 | |
| qwen3.8-flash | text | 2.21 / 8.99 | 52 | 0.051 | slow stream, bad tail |
| ... same | text+image | 3.18 / 9.88 | 61 | 0.065 | |
| glm-5.3-flash (reasoning low, forced) | text | 1.54 / 2.17 | 179 | 0.033 | wordy (42 words), provider varies |
| ... same | text+image | 2.25 / 2.81 | 139 | 0.051 | |

Full table with prompt/output tokens, providers seen and answer-length stats: run `python report.py results.json`.

## Quality notes (n=1 read per cell, plus length stats)
- All 9 configs answered in Russian with the English expansion kept in Latin; none drifted to English. All defined EBITDA correctly. With the image, every model pulled $0.6M / margin 14% from the slide.
- Brevity (system prompt asks for 2-3 sentences): gpt-6-luna 23-26 words (tightest), deepseek 31-40, qwen 36-38, haiku 34-44, gemini 38-44 (adds markdown bold despite the prompt), glm 42-43. All within the limit.
- Default-routed DeepSeek is a lottery of hosts (different quantisation/latency each call); pinning gives the stable numbers. Cost for sort=latency and :nitro is ~2x because the fast hosts charge more (`usage.cost`), still ~$0.1 per 100 asks.
- Prompt token counts differ: haiku 4.5k vs gpt-6-luna 3.0k for the same text (tokenizer).

## Recommendation
Default: **`deepseek/deepseek-v4.1-flash-1017`**, reasoning off: best TTFT p50/p95 in both variants (1.2-1.4 s, p95 <1.7 s), ~290 tok/s, vision included, $0.035-0.06 per 100 asks. Fallback / "reliable" option: **`anthropic/claude-haiku-5.5`** (1.5-2.2 s, tight p95, single provider) and **`openai/gpt-6-luna`** when the cheapest and shortest answers matter (text 1.65 s, image p95 3.9 s). The plain `deepseek-v4.1-flash` slug should not be the default unrouted (p95 6-12.5 s); if used, pin with `:nitro` / `sort=latency` (x2 cost). Avoid gemini-3.8-flash and glm-5.3-flash for the hot path (mandatory reasoning: more cost, slower with image) and qwen3.8-flash (52-61 tok/s, p95 9-10 s).

Caveats: one client location (no RU/US vantage points), n=8, one afternoon; provider pools shift hourly, so re-run `bench.py` before shipping. Prompt caching and reasoning-on were not tested. Total OpenRouter spend for this run about $0.2.
