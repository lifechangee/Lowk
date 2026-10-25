#!/usr/bin/env bash
# Fails when a worker build was tuned to the build host: inspects every CMakeCache.txt under the worker
# build trees. Run after tools/build-worker.sh / workers/lowk-embed-worker/build.sh (release-build.yml).
set -euo pipefail
cd "$(dirname "$0")/.."
mapfile -t caches < <(find workers/lowk-stt-worker/target workers/lowk-embed-worker/.build -name CMakeCache.txt 2>/dev/null)
[ "${#caches[@]}" -gt 0 ] || { echo "check-worker-isa: no CMakeCache.txt found (build the workers first)" >&2; exit 2; }
bad=0
for c in "${caches[@]}"; do
  grep -q '^GGML_NATIVE:BOOL=' "$c" || continue   # not a ggml tree
  for must_off in GGML_NATIVE GGML_AVX512 GGML_AVX512_VBMI GGML_AVX512_VNNI GGML_AVX512_BF16; do
    if grep -q "^$must_off:BOOL=ON" "$c"; then echo "check-worker-isa: $must_off=ON in $c" >&2; bad=1; fi
  done
  echo "check-worker-isa: ok $c"
done
exit $bad
