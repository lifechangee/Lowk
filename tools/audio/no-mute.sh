#!/usr/bin/env bash
# AUDIBLE, opt-in (plays a quiet system sound ~25 s on the CURRENT output; volume untouched).
# Regression for "capture silences playback": the output device's loopback energy must not drop
# when a Lowk tap (whole-system, then per-app) starts. Needs `swiftc`; run: tools/audio/no-mute.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
T=$(mktemp -d); trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$T"' EXIT
swiftc -O tools/audio/loopback.swift -o "$T/loopback" 2>/dev/null
BIN=$(cargo test -p lowk-audio --no-run --lib --message-format=json 2>/dev/null | python3 -c 'import sys,json
for l in sys.stdin:
    try: m=json.loads(l)
    except: continue
    if m.get("executable") and m.get("target",{}).get("kind")==["lib"]: print(m["executable"])' | tail -1)
(while true; do afplay /System/Library/Sounds/Submarine.aiff; done) & PLAYER=$!
sleep 1
meas() { "$T/loopback" 2 | python3 -c 'import sys,json; print(json.load(sys.stdin).get("db",-200))'; }
base=$(meas)
fail=0
check() { # name db
  python3 - "$1" "$base" "$2" <<'P' || fail=1
import sys; n,b,d=sys.argv[1],float(sys.argv[2]),float(sys.argv[3])
ok = b > -60 and d > b - 6
print(f"{'PASS' if ok else 'FAIL'} {n}: baseline {b:.1f} dB, with capture {d:.1f} dB"); sys.exit(0 if ok else 1)
P
}
LOWK_HOLD_SECS=6 "$BIN" --ignored hold_tap_open >/dev/null 2>&1 & H=$!; sleep 2.5
check "whole-system tap" "$(meas)"; wait $H || true
LOWK_HOLD_SECS=6 "$BIN" --ignored hold_tap_open >/dev/null 2>&1 & H=$!; sleep 2.5
check "second whole-system tap" "$(meas)"; wait $H || true
kill $PLAYER 2>/dev/null || true
exit $fail
