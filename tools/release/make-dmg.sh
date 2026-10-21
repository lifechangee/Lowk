#!/usr/bin/env bash
# Builds a .dmg and a .app.zip from the already ad-hoc-signed Lowk.app (ditto keeps the signature intact).
# Usage: tools/release/make-dmg.sh [out-dir]   (default: target/release/bundle/dist)
set -euo pipefail
cd "$(dirname "$0")/../.."
APP=target/release/bundle/macos/Lowk.app
OUT="${1:-target/release/bundle/dist}"
mkdir -p "$OUT"
ver="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
arch="$(uname -m)"
stage="$(mktemp -d)"; trap 'rm -rf "$stage"' EXIT
ditto "$APP" "$stage/Lowk.app"
ln -s /Applications "$stage/Applications"
hdiutil create -volname Lowk -srcfolder "$stage" -ov -format UDZO "$OUT/Lowk-$ver-macos-$arch.dmg"
ditto -c -k --keepParent "$APP" "$OUT/Lowk-$ver-macos-$arch.app.zip"
ls -l "$OUT"
