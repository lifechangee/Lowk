#!/usr/bin/env bash
# Sets the version everywhere: Cargo workspace (source of truth), ui/package.json, worker crates; refreshes lockfiles offline.
# Usage: tools/release/bump.sh 0.1.0   (or 0.1.0-rc.1)   -- `just bump 0.1.0`
set -euo pipefail
cd "$(dirname "$0")/../.."
v="${1:?usage: bump.sh <version>}"
[[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]] || { echo "version must look like 0.1.0 or 0.1.0-rc.1" >&2; exit 2; }
sed -i.bak '/^\[workspace.package\]/,/^\[/s/^version = ".*"/version = "'"$v"'"/' Cargo.toml
perl -0pi -e 's/^  "version": ".*?",/  "version": "'"$v"'",/m' ui/package.json
for f in workers/*/Cargo.toml; do
  sed -i.bak '/^\[package\]/,/^\[/s/^version = ".*"/version = "'"$v"'"/' "$f"
done
find . -name '*.bak' -not -path './node_modules/*' -not -path './target/*' -not -path './ui/node_modules/*' -delete
cargo update --workspace --offline >/dev/null
for f in workers/*/Cargo.toml; do cargo update --workspace --offline --manifest-path "$f" >/dev/null 2>&1 || true; done
tools/release/check-versions.sh
echo "Next: edit CHANGELOG.md, commit, tag v$v (docs/release.md)."
