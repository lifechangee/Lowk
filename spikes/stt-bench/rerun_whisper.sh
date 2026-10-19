#!/bin/bash
cd "$(dirname "$0")"; export HF_HOME=$PWD/models/hf HF_HUB_OFFLINE=1
.venv/bin/python harness/bench.py --engines whisper_cpp --clips 'audio/0*.wav' 'audio/fleurs*.wav' 'audio/synth*.wav' --cfg audio_ctx=768 --cfg stride_ms=1000 --cfg max_utt_s=14 --tag turbo-q5-ac768-v2
