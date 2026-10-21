#!/usr/bin/env bash
# Builds workers/lowk-embed-worker (llama.cpp `llama-server`, pinned tag) for the host target and
# stages it as the Tauri sidecar `binaries/lowk-embed-worker-<target-triple>[.exe]` (externalBin of
# tauri.bundle.conf.json), next to lowk-stt-worker. Needs git + CMake (Metal on macOS).
#   LOWK_EMBED_VULKAN=1   Windows/Linux GPU build (Vulkan SDK)
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
triple="$(rustc -vV | sed -n 's/^host: //p')"
ext=""; case "$triple" in *windows*) ext=".exe";; esac
src="workers/lowk-embed-worker/target/release/lowk-embed-worker$ext"
if [ ! -x "$src" ] || [ -n "${LOWK_REBUILD_EMBED:-}" ]; then
  workers/lowk-embed-worker/build.sh
fi
mkdir -p crates/lowk-app/binaries
cp "$src" "crates/lowk-app/binaries/lowk-embed-worker-$triple$ext"
ls -l "crates/lowk-app/binaries/lowk-embed-worker-$triple$ext"
