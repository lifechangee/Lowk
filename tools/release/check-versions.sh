#!/usr/bin/env bash
# Lint: every version in the repo equals the Cargo workspace version (the single source of truth).
#   tauri.conf.json has no "version" (inherits from Cargo); ui/package.json and the out-of-workspace worker crates must match.
# Usage: tools/release/check-versions.sh [tag]   (with a tag like v0.1.0-rc.1 it must equal the version too)
set -euo pipefail
cd "$(dirname "$0")/../.."
ver="$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' Cargo.toml | head -1)"
[ -n "$ver" ] || { echo "no [workspace.package] version in Cargo.toml" >&2; exit 1; }
bad=0
fail() { echo "VERSION MISMATCH: $1" >&2; bad=1; }
pkg="$(sed -n 's/^  "version": "\(.*\)",/\1/p' ui/package.json | head -1)"
[ "$pkg" = "$ver" ] || fail "ui/package.json is '$pkg', workspace is '$ver'"
for f in workers/*/Cargo.toml; do
  v="$(sed -n '/^\[package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' "$f" | head -1)"
  [ "$v" = "$ver" ] || fail "$f is '$v', workspace is '$ver'"
done
grep -q '"version"' crates/lowk-app/tauri.conf.json && fail "crates/lowk-app/tauri.conf.json must not set \"version\" (inherits from Cargo)"
for f in crates/*/Cargo.toml tools/xtask/Cargo.toml; do
  grep -q '^version.workspace = true' "$f" || fail "$f does not inherit version.workspace"
done
if [ $# -ge 1 ]; then
  [ "${1#v}" = "$ver" ] || fail "tag '$1' does not match version '$ver'"
fi
[ $bad = 0 ] && echo "versions OK: $ver"
exit $bad
