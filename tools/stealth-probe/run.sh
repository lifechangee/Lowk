#!/usr/bin/env bash
# Stealth capture-probe regression (macOS). Exit 0 = every scenario met its expectation.
#
#   tools/stealth-probe/run.sh [outdir]
#
# Builds the Swift probe and the `lowk-stealth` fixture (magenta OVERLAY + green MAIN,
# real window-policy code), drives the fixture through the scenarios of
# the earlier stealth spike's results plus a NEGATIVE CONTROL (exclusion off: the overlay MUST be
# visible, otherwise the probe itself is broken), and captures the screen four ways
# (SCK screenshot, SCK stream, legacy CGWindowList, screencapture CLI) per scenario.
# Needs Screen Recording permission for the terminal / host app running this script.
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$(mktemp -d)}"
mkdir -p "$OUT"
export PATH="$HOME/.cargo/bin:$PATH"

swiftc -O "$ROOT/tools/stealth-probe/probe.swift" -o "$OUT/probe" 2>/dev/null || { echo "swiftc failed"; exit 2; }
(cd "$ROOT" && cargo build -q -p lowk-stealth --example fixture) || exit 2
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/examples/fixture"

CTL="$OUT/fixture.ctl"; LOG="$OUT/fixture.log"; : > "$LOG"
"$BIN" "$CTL" "$LOG" >/dev/null 2>&1 &
PID=$!
cleanup() { echo quit > "$CTL" 2>/dev/null; sleep 0.4; kill "$PID" 2>/dev/null; }
trap cleanup EXIT
sleep 2

ctl() { echo "$1" > "$CTL"; sleep "${2:-0.7}"; }
FAILS=0
check() { # label expectation
  if "$OUT/probe" "$OUT" "$1" --owner fixture --expect "$2" > "$OUT/$1.txt" 2>&1; then
    echo "PASS  $1  ($2)"
  else
    echo "FAIL  $1  ($2)"; grep -E "FAIL" "$OUT/$1.txt" | sed 's/^/        /'; FAILS=$((FAILS + 1))
  fi
}
HID="overlay=hidden,main=visible"

ctl "overlay-show"; ctl "main-show"
check 1-both-open "$HID"
for _ in 1 2 3 4 5; do ctl "main-hide" 0.15; ctl "main-show" 0.15; done
check 2-main-hide-show "$HID"
ctl "overlay-move 120 200"
check 3-overlay-moved "$HID"
ctl "mode interactive"; check 4-interactive "$HID"; ctl "mode passive"
ctl "magnet 150 250 400 350" 1.5; check 5-after-magnet "$HID"
ctl "exclusion off"; check 6-NEGATIVE-CONTROL "overlay=visible,main=visible"
ctl "exclusion on";  check 7-exclusion-back-on "$HID"
ctl "panic"; check 8-panic-hidden "overlay=hidden,main=visible"
ctl "panic" 1.0; check 9-panic-restored "$HID"
ctl "overlay-hide"; check 10-overlay-hidden "overlay=hidden,main=visible"; ctl "overlay-show"
ctl "hide-main on"; check 11-hide-main-on "overlay=hidden,main=hidden"
ctl "hide-main off"; check 12-hide-main-off "$HID"
for _ in $(seq 1 50); do echo "main-hide" > "$CTL"; sleep 0.06; echo "main-show" > "$CTL"; sleep 0.06; done
sleep 0.5; check 13-main-stress-50x "$HID"

ctl "state"; grep -E "status=|selftest=|STATE" "$LOG" | tail -3
echo "results in $OUT"
[ "$FAILS" -eq 0 ] && echo "ALL PASS" || echo "$FAILS scenario(s) FAILED"
exit "$FAILS"
