#!/usr/bin/env bash
# Prints the release body: the CHANGELOG.md section for <version> (or a stub) + a placeholder for the Russian summary.
# Usage: tools/release/changelog-section.sh 0.1.0
set -euo pipefail
cd "$(dirname "$0")/../.."
v="${1:?version}"; base="${v%%-*}"
sec=""
if [ -f CHANGELOG.md ]; then
  for want in "$v" "$base"; do
    sec="$(awk -v v="$want" '
      /^## / { if (on) exit; h=$0; gsub(/[\[\]]/,"",h); if (h ~ "^## +v?" v "([ ]|$)") on=1; next }
      on { print }' CHANGELOG.md)"
    [ -n "$(echo "$sec" | tr -d '[:space:]')" ] && break || sec=""
  done
fi
[ -n "$sec" ] || sec="Release $v. (No CHANGELOG.md section found for this version.)"
printf '%s\n\n---\n\n## Кратко по-русски\n\n<!-- TODO(maintainer): short Russian summary before publishing -->\n\n' "$sec"
