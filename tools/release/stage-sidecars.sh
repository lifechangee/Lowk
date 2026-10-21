#!/usr/bin/env bash
# Stages both sidecars under crates/lowk-app/binaries/<name>-<target-triple>[.exe] and writes
# crates/lowk-app/tauri.release.conf.json (externalBin = STT worker + embedder). Used by the release-build
# workflow; independent of tauri.bundle.conf.json so it works whether or not main already lists the embedder.
#   LOWK_WORKER_FEATURES  STT worker cargo features (macOS default metal; Windows: vulkan | cuda | empty = CPU)
#   LOWK_EMBED_VULKAN     1 = Vulkan embedder (needs the Vulkan SDK); default CPU
set -euo pipefail
cd "$(dirname "$0")/../.."
tools/build-worker.sh
workers/lowk-embed-worker/build.sh
triple="$(rustc -vV | sed -n 's/^host: //p')"
ext=""; case "$triple" in *windows*) ext=".exe";; esac
cp "workers/lowk-embed-worker/target/release/lowk-embed-worker$ext" "crates/lowk-app/binaries/lowk-embed-worker-$triple$ext"
cat > crates/lowk-app/tauri.release.conf.json <<JSON
{ "bundle": { "externalBin": ["binaries/lowk-stt-worker", "binaries/lowk-embed-worker"] } }
JSON
ls -l crates/lowk-app/binaries
