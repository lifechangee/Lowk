#!/usr/bin/env bash
# Builds the embedding sidecar: upstream llama.cpp `llama-server` (pinned tag), static, without
# curl/openssl, as `target/release/lowk-embed-worker[.exe]`. The app talks to it over loopback HTTP
# (`/v1/embeddings`), see crates/lowk-index/src/embedder/local.rs.
# Not part of the Cargo workspace: needs git + CMake (+ Metal on macOS, Vulkan SDK for LOWK_EMBED_VULKAN=1).
set -euo pipefail
cd "$(dirname "$0")"
TAG="${LLAMA_CPP_TAG:-v0.7.0}"   # first line to bump; must know the `gemma-embedding2` architecture
SRC=".build/llama.cpp"
if [ ! -d "$SRC/.git" ]; then
  mkdir -p .build
  git clone --depth 1 --branch "$TAG" https://github.com/ggml-org/llama.cpp "$SRC"
fi
flags=(-DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DLLAMA_CURL=OFF -DLLAMA_OPENSSL=OFF
       -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_TOOLS=ON -DLLAMA_BUILD_SERVER=ON)
# Portable x86 baseline (see tools/worker-isa-flags.sh): never tune to the build machine.
case "$(uname -m)" in
  x86_64|AMD64|amd64|i686) read -r -a isa <<< "$(../../tools/worker-isa-flags.sh)"; flags+=("${isa[@]}");;
esac
case "$(uname -s)" in
  Darwin) flags+=(-DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON) ;;
  *) [ "${LOWK_EMBED_VULKAN:-0}" = 1 ] && flags+=(-DGGML_VULKAN=ON) ;;
esac
cmake -S "$SRC" -B .build/out "${flags[@]}"
cmake --build .build/out --config Release --target llama-server -j "$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)"
mkdir -p target/release
ext=""; case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) ext=".exe";; esac
bin="$(find .build/out -name "llama-server$ext" -type f | head -1)"
cp "$bin" "target/release/lowk-embed-worker$ext"
ls -l "target/release/lowk-embed-worker$ext"
