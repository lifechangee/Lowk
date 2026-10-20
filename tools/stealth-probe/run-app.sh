#!/usr/bin/env bash
# Stealth capture-probe regression against the REAL Lowk app (macOS, debug build).
#
#   tools/stealth-probe/run-app.sh [outdir]
#
# Builds the UI and the app (embedded assets), launches it in probe mode
# (LOWK_PROBE=1: overlay = solid magenta page, main = solid green page; LOWK_PROBE_CTL: scenario
# control file), drives the scenarios of docs/ARCHITECTURE.md M0 exit criterion 1 and captures the
# screen four ways per scenario. Needs Screen Recording for the terminal / host app.
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$(mktemp -d)}"
mkdir -p "$OUT/data"
export PATH="$HOME/.local/node/current/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"

swiftc -O "$ROOT/tools/stealth-probe/probe.swift" -o "$OUT/probe" 2>/dev/null || { echo "swiftc failed"; exit 2; }
(cd "$ROOT" && pnpm --dir ui build >/dev/null && cargo build -q -p lowk-app --features custom-protocol) || exit 2
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/lowk"

CTL="$OUT/app.ctl"; LOG="$OUT/app.log"; : > "$CTL"; : > "$LOG"
printf '[onboarding]\ndone = true\nrecording_notice_ack = true\n[vault]\npath = "%s/vault"\n' "$OUT" > "$OUT/data/settings.toml"
LOWK_DATA_DIR="$OUT/data" LOWK_PROBE=1 LOWK_PROBE_CTL="$CTL" "$BIN" >"$OUT/stdout.log" 2>&1 &
PID=$!
cleanup() { echo quit > "$CTL" 2>/dev/null; sleep 0.6; kill "$PID" 2>/dev/null; }
trap cleanup EXIT
sleep 4
front() { osascript -e 'tell application "System Events" to set frontmost of first process whose name is "lowk" to true' >/dev/null 2>&1; sleep 0.6; }
ctl() { echo "$1" >> "$CTL"; sleep "${2:-0.8}"; case "$1" in main-show) front;; esac; }
FAILS=0
check() { # label expectation
  [ "${2#*main=visible}" != "$2" ] && front   # the real app must be frontmost, or other windows cover the green page
  if "$OUT/probe" "$OUT" "$1" --owner lowk --expect "$2" > "$OUT/$1.txt" 2>&1; then
    echo "PASS  $1  ($2)"
  else
    echo "FAIL  $1  ($2)"; grep -E "FAIL" "$OUT/$1.txt" | sed 's/^/        /'; FAILS=$((FAILS + 1))
  fi
}
HID="overlay=hidden,main=visible"

front
check 0-main-only "overlay=hidden,main=visible"
ctl "overlay-show"
check 1-both-open "$HID"
for _ in 1 2 3 4 5; do ctl "main-hide" 0.3; ctl "main-show" 0.5; done
check 2-main-hide-show "$HID"
ctl "main-destroy" 1.0; check 3a-main-destroyed "overlay=hidden,main=hidden"
ctl "main-show" 2.0;    check 3b-main-recreated "$HID"
ctl "main-destroy" 1.0; ctl "main-show" 2.0; check 3c-main-recreated-again "$HID"
ctl "overlay-move 120 200"; check 4-overlay-moved "$HID"
ctl "mode interactive"; check 5-interactive "$HID"; ctl "mode passive"
ctl "magnet 150 250 400 350" 1.5; check 6-after-magnet "$HID"
ctl "policy-flip" 1.5; check 7-policy-flip "$HID"
ctl "exclusion off"; check 8-NEGATIVE-CONTROL "overlay=visible,main=visible"
ctl "exclusion on";  check 9-exclusion-back-on "$HID"
ctl "panic"; check 10-panic-hidden "overlay=hidden,main=visible"
ctl "panic" 1.5; check 11-panic-restored "$HID"
ctl "overlay-hide"; check 12-overlay-hidden "overlay=hidden,main=visible"; ctl "overlay-show"
ctl "hide-main on"; check 13-hide-main-on "overlay=hidden,main=hidden"
ctl "hide-main off"; check 14-hide-main-off "$HID"
for _ in $(seq 1 25); do echo "main-hide" >> "$CTL"; sleep 0.2; echo "main-show" >> "$CTL"; sleep 0.2; done; front
sleep 1; check 15-main-stress-25x "$HID"

ctl "state"; tail -3 "${CTL%.ctl}.log"
echo "results in $OUT"
[ "$FAILS" -eq 0 ] && echo "ALL PASS" || echo "$FAILS scenario(s) FAILED"
exit "$FAILS"
