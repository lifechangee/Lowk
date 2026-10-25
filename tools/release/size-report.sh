#!/usr/bin/env bash
# Markdown installer size table (targets: dmg <= 25 MB, NSIS <= 20 MB). Over target = warning only.
# Usage: tools/release/size-report.sh <dir>
dir="${1:?dir}"
echo "### Installer sizes"; echo; echo "| file | size | target | |"; echo "|---|---|---|---|"
for f in "$dir"/*.dmg "$dir"/*-setup.exe; do
  [ -f "$f" ] || continue
  case "$f" in *.dmg) t=25;; *) t=20;; esac
  mb=$(( ($(wc -c <"$f") + 1048575) / 1048576 ))
  if [ "$mb" -gt "$t" ]; then s="OVER target"; echo "::warning::$(basename "$f") is ${mb} MB (target <= ${t} MB)" >&2; else s="ok"; fi
  echo "| $(basename "$f") | ${mb} MB | <= ${t} MB | $s |"
done
exit 0
