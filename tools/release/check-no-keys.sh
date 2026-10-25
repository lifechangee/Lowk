#!/usr/bin/env bash
# CI guard: no private key material may be tracked (updater signing keys, PEM/minisign secrets).
set -u
cd "$(dirname "$0")/../.."
bad=0
if git ls-files | grep -Ei '\.key$|\.key\.|(^|/)\.tauri/|id_(rsa|ed25519)$|\.pem$|\.p12$'; then
  echo "KEY GUARD: key-like file is tracked (above)" >&2; bad=1
fi
# Tauri/minisign secret keys are base64 of "untrusted comment: minisign encrypted secret key" -> starts with dW50cnVzdGVk; PEM headers.
if git grep -nIE 'BEGIN [A-Z ]*PRIVATE KEY|minisign encrypted secret key|^dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIGVuY3J5cHRlZCBzZWNyZXQ' -- . ':!tools/release/check-no-keys.sh' ':!docs/release.md'; then
  echo "KEY GUARD: private key material found in tracked files (above)" >&2; bad=1
fi
[ $bad = 0 ] && echo "no private keys tracked"
exit $bad
