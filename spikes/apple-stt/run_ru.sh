#!/bin/bash
# Sequential real-time run of the 20 owner clips through DictationTranscriber ru_RU.
# usage: run_ru.sh OUTDIR
cd "$(dirname "$0")"; A=../stt-bench/audio; O=$1; mkdir -p "$O"
for f in $A/0*.m4a; do b=$(basename $f .m4a); .build/release/applestt run $f --engine dictation --locale ru-RU --download --out $O/dictation_ru_$b.jsonl 2>>$O/err.log; done
