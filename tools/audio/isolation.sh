#!/usr/bin/env bash
# Audio isolation test (macOS): two afplay processes play different tones at once; capturing
# one chosen process must contain only its tone, the whole-system tap must contain both.
# Needs the System Audio Recording permission for the host terminal/app. Exit 0 = pass.
#   tools/audio/isolation.sh [work-dir]
set -euo pipefail
cd "$(dirname "$0")/../.."
export PATH="$HOME/.cargo/bin:$PATH"
W="${1:-${TMPDIR:-/tmp}/lowk-isolation}"
mkdir -p "$W"
cargo build --release -p lowk-audio --example capture >/dev/null
B=target/release/examples/capture
$B gen-tones "$W" >/dev/null

level() { # file freq -> amplitude
  $B analyze "$1" | sed -E "s/.*tone${2}=([0-9.]+).*/\1/"
}
check() { # name file want440(1/0) want880(1/0)
  local a b
  a=$(level "$2" 440); b=$(level "$2" 880)
  echo "  $1: tone440=$a tone880=$b"
  awk -v a="$a" -v b="$b" -v w4="$3" -v w8="$4" 'BEGIN{
    ok4 = w4 ? a > 0.15 : a < 0.03; ok8 = w8 ? b > 0.15 : b < 0.03; exit !(ok4 && ok8) }' || { echo "  FAIL: $1"; return 1; }
}

afplay -v 1 "$W/tone_440.wav" & P440=$!
afplay -v 1 "$W/tone_880.wav" & P880=$!
trap 'kill $P440 $P880 2>/dev/null || true' EXIT
sleep 1.5
fail=0
$B capture --app "$P440" --no-mic --duration 4 --out "$W/a440" 2>/dev/null
check "app=440 player" "$W/a440/them.wav" 1 0 || fail=1
$B capture --app "$P880" --no-mic --duration 4 --out "$W/a880" 2>/dev/null
check "app=880 player" "$W/a880/them.wav" 0 1 || fail=1
$B capture --system --no-mic --duration 4 --out "$W/sys" 2>/dev/null
check "whole system" "$W/sys/them.wav" 1 1 || fail=1
kill "$P440" 2>/dev/null || true
sleep 0.5
$B capture --system --no-mic --duration 4 --out "$W/sys2" 2>/dev/null
check "system, 440 stopped" "$W/sys2/them.wav" 0 1 || fail=1
if [ "$fail" = 0 ]; then echo "ISOLATION PASS"; else echo "ISOLATION FAIL"; exit 1; fi
