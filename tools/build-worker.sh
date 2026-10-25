#!/usr/bin/env bash
# Builds workers/lowk-stt-worker for the host target and stages it as the Tauri sidecar
# (`externalBin` = binaries/lowk-stt-worker-<target-triple>[.exe]) so `cargo tauri build`
# (config tauri.bundle.conf.json) ships it inside the app bundle.
#   LOWK_WORKER_FEATURES   cargo features of the worker; default: metal on macOS, none elsewhere
#                          (CPU). Windows GPU builds: LOWK_WORKER_FEATURES=vulkan or cuda.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
triple="$(rustc -vV | sed -n 's/^host: //p')"
ext=""; case "$triple" in *windows*) ext=".exe";; esac
case "$(uname -s)" in
  Darwin) feat="${LOWK_WORKER_FEATURES-metal}";;
  *) feat="${LOWK_WORKER_FEATURES-}";;
esac
# Portable CPU baseline on x86 (regression: a native build on a runner with AVX-512 crashed with
# STATUS_ILLEGAL_INSTRUCTION on CPUs without it). Haswell tier: AVX2 + FMA + F16C + BMI2; no AVX-512.
# Apple Silicon is already pinned by the crate.
case "$(uname -m)" in
  x86_64|AMD64|amd64|i686)
    export TRANSCRIBE_CMAKE_ARGS="${TRANSCRIBE_CMAKE_ARGS:-} $(tools/worker-isa-flags.sh)";;
esac
args=(--release --manifest-path workers/lowk-stt-worker/Cargo.toml --no-default-features)
[ -n "$feat" ] && args+=(--features "$feat")
cargo build "${args[@]}"
mkdir -p crates/lowk-app/binaries
cp "workers/lowk-stt-worker/target/release/lowk-stt-worker$ext" "crates/lowk-app/binaries/lowk-stt-worker-$triple$ext"
ls -l "crates/lowk-app/binaries/lowk-stt-worker-$triple$ext"
