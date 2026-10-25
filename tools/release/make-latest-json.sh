#!/usr/bin/env bash
# Writes latest.json for the Tauri updater from the files in <dir>. Needs jq. Only platforms whose update bundle AND .sig exist are listed.
# Usage: tools/release/make-latest-json.sh <dir> <version> <tag> <owner/repo> [notes-file]
set -euo pipefail
dir="$1"; ver="$2"; tag="$3"; repo="$4"; notes="${5:-}"
base="https://github.com/$repo/releases/download/$tag"
platforms='{}'
add() { # key file
  local f="$dir/$2"
  [ -f "$f" ] && [ -f "$f.sig" ] || { echo "skip $1: $2(.sig) missing" >&2; return 0; }
  platforms="$(jq --arg k "$1" --arg u "$base/$2" --rawfile s "$f.sig" '. + {($k): {signature: ($s|rtrimstr("\n")), url: $u}}' <<<"$platforms")"
}
add darwin-aarch64 "Lowk-$ver-macos-arm64.app.tar.gz"
add darwin-x86_64 "Lowk-$ver-macos-x86_64.app.tar.gz"
w="$(cd "$dir" && ls *-setup.exe 2>/dev/null | head -1 || true)"
[ -n "$w" ] && add windows-x86_64 "$w"
[ "$platforms" != '{}' ] || { echo "no signed update bundles; latest.json not written" >&2; exit 3; }
jq -n --arg v "$ver" --arg n "$(cat "${notes:-/dev/null}")" --arg d "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson p "$platforms" \
  '{version:$v, notes:$n, pub_date:$d, platforms:$p}' > "$dir/latest.json"
cat "$dir/latest.json"
