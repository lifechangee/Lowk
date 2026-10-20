# STT bake-off, first pass (MacBook Air M4, 24 GB, macOS 27.0.1)

**Status: preliminary.** One machine, one speaker pair (the owner + one partner), 20 short real clips, references taken from the scripts in `docs/test-audio/SCRIPTS.md` (people deviated from the scripts, so WER is approximate and inflated by script differences, number formatting "42" vs "сорок два", and ё/е). Compare engines against each other, not against absolute numbers.

**Timing noise:** a UI-mockup agent and a computer-use test agent were running on the same Mac during all runs. CPU/GPU contention inflates compute times, so Voxtral's "can't keep up" result may be partly the load (RTF ~1.5 observed; an idle machine was not tried). Whisper and Nemotron were less affected but numbers are indicative only.

## Results

Test sets: real = owner recordings 01-05 x {me, them} x {quiet, noisy}, 16 kHz mono, 160 ms chunks paced in real time. "me" = long monologue turns (Russian with heavy EN jargon, scenario 05 mostly English), "them" = short turns. Secondary = 3 FLEURS ru_ru clips (CC-BY, 5 utterances glued, ~60-80 s) + 1 synthetic macOS-`say` RU/EN code-switch clip (synthetic, not real speech). Raw per-clip JSON in `results/`, full generated tables in `results/report.md`.

| engine | set | clips | WER | CER | word lag p50/p90 (s) | onset->1st partial p50 (s) | final after EOS (s) | RTF (compute) | peak RAM MB |
|---|---|---|---|---|---|---|---|---|---|
| nemotron_mlx:bf16-auto | real noisy (_n) | 10 | 0.316 | 0.230 | 0.40/1.54 | 1.39 | 0.14 | 0.15 | 1650 |
| voxtral_mlx:4bit-480ms | real noisy (_n) | 10 | 0.188 | 0.130 | 27.78/31.02 | 1.54 | 28.58 | 1.60 | 3446 |
| whisper_cpp:turbo-q5-ac768-v2 | real noisy (_n) | 10 | 0.287 | 0.186 | 1.68/4.07 | 1.01 | 1.69 | 0.97 | 983 |
| nemotron_mlx:bf16-auto | real quiet (_q) | 10 | 0.309 | 0.215 | 0.93/2.11 | 1.24 | 0.14 | 0.14 | 1650 |
| voxtral_mlx:4bit-480ms | real quiet (_q) | 10 | 0.203 | 0.140 | 30.05/32.76 | 1.56 | 30.49 | 1.68 | 3583 |
| whisper_cpp:turbo-q5-ac768-v2 | real quiet (_q) | 10 | 0.233 | 0.152 | 2.17/4.20 | 0.85 | 1.51 | 0.96 | 996 |
| nemotron_mlx:bf16-auto | secondary (FLEURS/synthetic) | 4 | 0.108 | 0.052 | 0.76/1.44 | 1.34 | 0.14 | 0.14 | 1651 |
| voxtral_mlx:4bit-480ms | secondary (FLEURS/synthetic) | 4 | 0.046 | 0.013 | 29.80/33.79 | 1.44 | 30.67 | 1.60 | 3518 |
| whisper_cpp:turbo-q5-ac768-v2 | secondary (FLEURS/synthetic) | 4 | 0.159 | 0.108 | 2.15/4.01 | 1.65 | 1.35 | 0.93 | 994 |


Columns: *word lag* = time from a word's (estimated) spoken end to the moment it first appeared in the stream, p50/p90 (end times are estimated by spreading reference words over VAD-active speech, no forced aligner, so +-0.3 s); *onset -> 1st partial* = speech burst start to first new word; *final after EOS* = time after the last audio chunk until the engine returned its final text (for Voxtral this is the backlog of a model that is slower than real time); *RTF (compute)* = model busy time / audio duration; *peak RAM* = harness + engine process RSS.

Configs: Whisper = large-v3-turbo q5_0, whisper.cpp Metal (no CoreML encoder), `-ac 768`, Silero VAD, partial re-decode every 1 s, LocalAgreement-2, 14 s utterance cap, language auto. Nemotron = `mlx-community/nemotron-3.5-asr-streaming-0.6b` bf16 (MLX, mlx-audio), language auto-detect, default chunking. Voxtral = `mlx-community/Voxtral-Mini-4B-Realtime-2602-4bit`, delay 480 ms. 6/8-bit Voxtral and Nemotron 8-bit were not run (time).

## English terms inside Russian speech

"exact" = the Latin word matches the reference at the aligned position. "Cyrillic" = rendered by transliteration (e.g. `ебита`, `роллбэк`), which is often readable but breaks search/term matching.

### English terms: RU with EN jargon (01-04, real; quiet+noisy)

| engine | exact Latin | Cyrillic transliteration | other mismatch | missing | exact % |
|---|---|---|---|---|---|
| nemotron_mlx:bf16-auto | 46 | 143 | 27 | 0 | 21% |
| voxtral_mlx:4bit-480ms | 123 | 72 | 21 | 0 | 57% |
| whisper_cpp:turbo-q5-ac768-v2 | 117 | 66 | 33 | 0 | 54% |

### English terms: English-heavy with RU inserts (05, real)

| engine | exact Latin | Cyrillic transliteration | other mismatch | missing | exact % |
|---|---|---|---|---|---|
| nemotron_mlx:bf16-auto | 186 | 22 | 55 | 7 | 69% |
| voxtral_mlx:4bit-480ms | 242 | 8 | 18 | 2 | 90% |
| whisper_cpp:turbo-q5-ac768-v2 | 206 | 44 | 19 | 1 | 76% |

### Per-term rendering (01-04; ok = kept in Latin, otherwise what the engine wrote)

| term | nemotron_mlx:bf16-auto | voxtral_mlx:4bit-480ms | whisper_cpp:turbo-q5-ac768-v2 |
|---|---|---|---|
| rate | 0/6 -> reid; right batterry | 4/6 -> бернрейд; берн рейд | 3/6 -> ride; arr |
| user | 0/4 -> юзер; юн дизерс интервью с | 0/4 -> юзер; юндизерс интервью с | 2/4 -> users; юзер |
| metric | 2/4 -> примари метрик | 3/4 -> метрик | 3/4 -> northstarmetric |
| maps | 2/4 -> оффлайн мапс | 4/4 | 4/4 |
| month | 1/4 -> манфаромат мафлин; math | 3/4 -> aromat | 3/4 -> or amount матери берем 3 |
| burn | 0/4 -> манфаромат мафлин; берn | 2/4 -> бернрейд; берн рейд | 0/4 -> or amount матери берем 3; музыка музыка берн 3 |
| ltv | 3/4 -> лтв | 4/4 | 2/4 -> 8 лтв; юнит экономикс лтв |
| cac | 2/4 -> сас | 2/4 -> sas 240; saas | 2/4 -> sas 240; сас |
| latency | 0/4 -> лайтенсии; лайтенсии найдпоинт слэш | 3/4 -> лейпингом | 3/4 -> стоит принцы |
| sla | 2/4 -> слай; сла | 4/4 | 4/4 |
| fintech | 0/2 -> pintech startip; финтеч | 0/2 -> пентек; финтедж | 1/2 -> pintech |
| product | 0/2 -> продукт | 0/2 -> продукт дискавери | 2/2 |
| discovery | 2/2 | 0/2 -> продукт дискавери | 2/2 |
| interviews | 0/2 -> юн дизерс интервью с; юзер интервью с | 0/2 -> юндизерс интервью с; юзер интервью | 2/2 |
| prd | 1/2 -> пирди | 2/2 | 1/2 -> aprd |
| flow | 0/2 -> запускали новую флоу; флоу активышн райт вырас | 2/2 | 1/2 -> запускали новую флоу |
| activation | 1/2 -> флоу активышн райт вырас | 2/2 | 2/2 |
| retention | 1/2 -> рентеншон dirty | 2/2 | 1/2 -> рентеншн |
| d30 | 0/2 -> рентеншон dirty; до 30 | 1/2 -> до 30 | 1/2 -> до 30 |
| cohort | 0/2 -> кухортам; кохортам | 0/2 -> кухортам; кохардам | 0/2 -> кухортам; кохортам |
| ads | 0/2 -> и цас; адс | 1/2 -> ecs | 1/2 -> itsas |
| intent | 0/2 -> интент | 2/2 | 2/2 |
| north | 1/2 -> nothar | 1/2 -> node | 1/2 -> northstarmetric |
| star | 1/2 -> nothar | 2/2 | 1/2 -> northstarmetric |
| engaged | 2/2 | 2/2 | 2/2 |
| users | 2/2 | 2/2 | 2/2 |
| rice | 0/2 -> rais; райс | 0/2 -> rise | 0/2 -> райс а; rise |
| ice | 0/2 -> is; айс | 2/2 | 1/2 -> айс |

Observations on terms: Nemotron transliterates most jargon into Cyrillic ("юзер интервью", "лайтенсии", "ебита", "пирди") and sometimes mangles it; Voxtral and Whisper keep far more terms in Latin. Systematic misses for all engines: `cohort` -> "кохорта", `RICE`, `ok` ("okay"), `D30`, `CAC` ("сас"). Whisper produced a few garbage insertions in noisy "me" audio ("month over month" -> "month or amount", "burn rate" -> "музыка"). Whole-clip hallucinations/duplications of Whisper were fixed in the harness (sentence duplication on trailing silence) before the reported v2 run. Voxtral did best on the English-heavy scenario (90% exact).

## Other observations

- **Nemotron 3.5 (MLX bf16)** is the only engine that is truly streaming here: compute RTF 0.14, final text 0.14 s after speech ends, word lag p50 ~0.4-0.9 s, 1.65 GB RSS. Quality is the weakest of the three on real RU+EN speech (WER 0.31 quiet / 0.32 noisy) and jargon handling is poor. Nothing published about RU code-switching, and this confirms it is its weak spot. Licence: the MLX card says "nvidia-open-model-license" (the research doc said OpenMDW) -> verify before shipping.
- **Voxtral Realtime 4B 4-bit** gave the best text (WER 0.20 quiet, 0.19 noisy; CER 0.14; best on terms), but on this (loaded) M4 Air it ran at compute RTF ~1.6, i.e. slower than real time: backlog grows linearly (final text ~30 s after a 60 s clip ends). Not usable as a live engine on an M4 Air here; peak RAM 3.4-3.6 GB. Might work on Pro/Max chips or an idle machine, and with a smaller delay window skipped frames; untested.
- **Whisper turbo + VAD + LocalAgreement** works end to end: word lag p50 ~2 s, p90 ~4 s, finals ~1.5 s after EOS, WER 0.23-0.29, 1 GB RAM. But compute RTF ~0.96 for ONE stream: the 30 s encoder dominates (~1-2 s per re-decode even with `-ac 768`; partial re-decode is the cost). Two simultaneous streams (mic + system) would not fit. A CoreML encoder build (not attempted) would help a lot. The <1 s latency target is not reachable with this design on an M4 Air.
- "them" clips are short and cleaner (WER 0.14-0.26) than "me" clips (0.25-0.37); noise (`_n`) hurt less than expected, probably because it was moderate.
- Cloud adapters (Soniox stt-rt-v5, Deepgram Nova-3 `multi`) are implemented in `adapters/cloud.py` from vendor docs but untested: no API keys, runs were skipped.

## Recommendation (provisional)

1. **Live default on Mac: Nemotron 3.5 streaming for latency/footprint, but pair it with a post-utterance correction pass** (Whisper turbo or Voxtral over finished utterances) that re-renders jargon; alone its English-term handling (21% exact in RU speech) is not good enough for the product's core use case.
2. **Cloud default (Soniox, Deepgram as the code-switch alternative) should be benchmarked next with keys**; given local results, cloud likely wins on latency x quality for code-switched RU/EN.
3. Whisper stays as the "accurate" engine / final-quality pass; only viable as a single stream on an M4 Air without CoreML.
4. Voxtral 4-bit is the quality leader but needs a stronger chip or a faster runtime; re-test on idle M4 Pro/Max or the 6-bit/8-bit builds before ruling out.
5. Before deciding: rerun on an idle machine, with a second speaker pair, with Nemotron `language=ru-RU` forced and a Whisper initial prompt of jargon terms (both supported via `--cfg language=... prompt=...`), and with a CoreML Whisper encoder.

## How to run

```bash
export PATH=$HOME/.local/bin:$PATH
uv venv --python 3.12 .venv && uv pip install -p .venv numpy soundfile soxr jiwer huggingface_hub psutil websockets requests pyarrow mlx-audio onnxruntime
# whisper.cpp (Metal) + model + VAD
git clone --depth 1 https://github.com/ggml-org/whisper.cpp engines/whisper.cpp
(cd engines/whisper.cpp && cmake -B build -DGGML_METAL=ON -DCMAKE_BUILD_TYPE=Release && cmake --build build -j8 --target whisper-server)   # cmake via `uv tool install cmake`
curl -L -o models/ggml-large-v3-turbo-q5_0.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin
curl -L -o models/silero_vad.onnx https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx
# MLX models download on first use into models/hf (set HF_HOME=$PWD/models/hf; HF_HUB_DISABLE_XET=1 helped when downloads stalled)
.venv/bin/python harness/bench.py --engines whisper_cpp,nemotron_mlx,voxtral_mlx --clips 'audio/0*.wav' --chunk-ms 160 --tag mytag
.venv/bin/python harness/make_report.py results      # tables
./run_all.sh                                          # the whole first-pass matrix (~1.5 h)
```
Engine options via `--cfg k=v` (e.g. `--cfg model=mlx-community/...-8bit`, `--cfg language=ru-RU`, `--cfg prompt="EBITDA, KPI"`, `--cfg audio_ctx=768`). Cloud: set `SONIOX_API_KEY` / `DEEPGRAM_API_KEY`, add `--engines soniox,deepgram` (otherwise skipped).

## Adding new recordings

Drop `NAME.wav` (any rate/channels; resampled to 16 kHz mono) and `NAME.txt` (reference text) into `audio/`. For m4a, `harness/prepare_real.py` converts with `afconvert` and builds references from `docs/test-audio/SCRIPTS.md` (`NN_{me,them}_{q,n}`). Public clips: `harness/prepare_data.py fleurs <parquet>` / `synth`. New engine: subclass `harness/base.py:Engine` in `adapters/` and register it in `ENGINES` in `harness/bench.py`.

## Layout / caveats

`harness/` runner, metrics, report; `adapters/` engines; `results/` JSON per run (`old/` = superseded runs: whisper v1 had a duplicated-sentence bug, `prelim/` = smoke tests); `run_all.sh`, `rerun_whisper.sh`. Weights, `engines/`, `.venv`, and the HF cache were deleted after the run (about 9 GB downloaded; see sizes below) and are gitignored. Known limits: word-lag uses estimated word times; term matching is alignment-based and counts valid transliterations as mismatches; the 'me'/'them' streams were run separately, not concurrently.

## Облако (Deepgram Nova-3 multilingual, Soniox)

Run 2026-10-20 on the same 20 real clips (01-05 x me/them x q/n), 160 ms chunks paced in real time, same harness and references as above (so the same script-deviation noise in WER). Machine was not idle (other agents running), but cloud compute is off-box, so only network/RTT noise applies. Raw JSON: `results/*deepgram-nova3-multi.json` (includes per-word lags).

**Soniox: not measured. Error class = billing.** The WebSocket accepts the key (no auth error) and then returns `error_code 402, organization_balance_exhausted` ("Organization balance exhausted. Please either add funds manually or enable autopay"). The account has no credit; the adapter code is unchanged and untested beyond the handshake. Re-run `--engines soniox` after topping up (the adapter also swallows server errors silently: it printed WER 1.0 with no message, worth fixing).

Deepgram config: `model=nova-3&language=multi`, linear16 16 kHz, interim_results, smart_format, endpointing 300 ms.

| engine | set | clips | WER | CER | word lag p50/p90/p95 (s) | onset->1st partial p50 (s) | final after EOS p50 (s) |
|---|---|---|---|---|---|---|---|
| deepgram nova-3 multi | real quiet (_q) | 10 | 0.270 | 0.172 | 0.81/2.86/3.63 | 1.11 | 0.36 |
| deepgram nova-3 multi | real noisy (_n) | 10 | 0.313 | 0.228 | 0.27/2.02/2.76 | 1.11 | 0.38 |
| deepgram nova-3 multi | all 20 | 20 | 0.291 | 0.200 | 0.59/2.59/3.34 | 1.11 | 0.36 |

(Lag percentiles are pooled over all words of all clips; the local rows above are median-of-clip p50 / p90, so only roughly comparable. Word lag is relative to estimated spoken-end times, +-0.3 s; the p90+ tail is dominated by the long "me" monologues where Deepgram holds words until an endpoint/final.)

### English terms inside Russian speech (Deepgram)

| set | exact Latin | Cyrillic transliteration | other mismatch | missing | exact % |
|---|---|---|---|---|---|
| RU with EN jargon (01-04) | 123 | 37 | 33 | 23 | 57% |
| English-heavy (05) | 215 | 2 | 25 | 28 | 80% |

Same exact-term rate as Voxtral on 01-04 (57%) and Whisper (54%), far better than Nemotron (21%); on 05 between Whisper (76%) and Voxtral (90%). Cost ~ $0.0077/min streamed, about $0.15 for this run.

### Updated recommendation (local vs cloud)

| role | engine | why |
|---|---|---|
| Cloud default (works today) | Deepgram Nova-3 `multi` | WER 0.27-0.31 (on par with best local), word lag p50 ~0.6 s, final 0.36 s, 57%/80% exact EN terms, no RAM/CPU; needs network + user key (BYOK) |
| Cloud alternative | Soniox stt-rt-v5 | Unmeasured: account balance exhausted (402). Re-test once funded before deciding |
| Local live default | Nemotron 3.5 streaming + correction pass | Only truly real-time local engine (final 0.14 s) but weak on jargon (21% exact EN) |
| Local accurate/final pass | Whisper turbo q5 (single stream) | WER 0.23-0.29, lag ~2 s |
| Local quality leader (needs faster chip) | Voxtral 4-bit | WER 0.19-0.20, 90% on 05, but RTF 1.6 on M4 Air |

Net: for latency x quality on code-switched RU/EN, Deepgram multi matches Voxtral on terms and Whisper/Nemotron on WER at sub-second lag, so it is the default when the user is online with a key; Nemotron+correction stays the offline/private mode. Caveat: WER differences of 0.02-0.04 are within script-deviation noise.
