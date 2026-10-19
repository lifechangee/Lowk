#!/bin/bash
# Full first-pass matrix (real-time paced; ~1.5 h). Usage: ./run_all.sh
cd "$(dirname "$0")"; export HF_HOME=$PWD/models/hf HF_HUB_OFFLINE=1
PY=.venv/bin/python
$PY harness/bench.py --engines whisper_cpp --clips 'audio/0*.wav' 'audio/fleurs*.wav' 'audio/synth*.wav' --cfg audio_ctx=768 --cfg stride_ms=1000 --cfg max_utt_s=14 --tag turbo-q5-ac768
$PY harness/bench.py --engines nemotron_mlx --clips 'audio/0*.wav' 'audio/fleurs*.wav' 'audio/synth*.wav' --tag bf16-auto
$PY harness/bench.py --engines voxtral_mlx --clips 'audio/0*.wav' 'audio/fleurs*.wav' 'audio/synth*.wav' --tag 4bit-480ms
$PY harness/bench.py --engines soniox,deepgram --clips 'audio/0*.wav'
