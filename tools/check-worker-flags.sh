#!/usr/bin/env bash
# Static guard: both worker builds must use the portable x86 baseline (regression: workers built on an
# AVX-512 runner died with STATUS_ILLEGAL_INSTRUCTION on older CPUs).
set -euo pipefail
cd "$(dirname "$0")/.."
tools/worker-isa-flags.sh | grep -q -- '-DGGML_NATIVE=OFF' || { echo "worker-isa-flags.sh must set GGML_NATIVE=OFF" >&2; exit 1; }
tools/worker-isa-flags.sh | grep -q -- '-DGGML_AVX512=OFF' || { echo "worker-isa-flags.sh must set GGML_AVX512=OFF" >&2; exit 1; }
grep -q worker-isa-flags.sh tools/build-worker.sh || { echo "build-worker.sh must use worker-isa-flags.sh" >&2; exit 1; }
grep -q worker-isa-flags.sh workers/lowk-embed-worker/build.sh || { echo "embed build.sh must use worker-isa-flags.sh" >&2; exit 1; }
