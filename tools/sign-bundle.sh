#!/usr/bin/env bash
# Signs the macOS bundle so TCC (Microphone, Screen Recording, Input Monitoring, Accessibility, System
# Audio) recognises it as the SAME app across rebuilds, and the signing Identifier equals the bundle id.
#   LOWK_SIGN_IDENTITY  name of a code-signing certificate in the login keychain ("Lowk Dev", see
#                       docs/setup/macos-signing.md). Unset -> ad-hoc signature with an explicit
#                       identifier and a designated requirement pinned to the bundle id.
# Usage: tools/sign-bundle.sh [path/to/Lowk.app]
set -euo pipefail
cd "$(dirname "$0")/.."
APP="${1:-target/release/bundle/macos/Lowk.app}"
[ -d "$APP" ] || { echo "no bundle at $APP (run: just build)" >&2; exit 1; }
BID="$(/usr/bin/plutil -extract CFBundleIdentifier raw "$APP/Contents/Info.plist")"
IDENT="${LOWK_SIGN_IDENTITY:--}"

if [ "$IDENT" != "-" ] && ! security find-identity -p codesigning | grep -q "\"$IDENT\""; then
  echo "LOWK_SIGN_IDENTITY='$IDENT' is not a valid code-signing identity in your keychains." >&2
  echo "Create it once: docs/setup/macos-signing.md" >&2
  exit 1
fi

# Nested executables first (the STT sidecar), each with its own stable identifier.
for f in "$APP"/Contents/MacOS/*; do
  [ -f "$f" ] || continue
  name="$(basename "$f")"
  main="$(/usr/bin/plutil -extract CFBundleExecutable raw "$APP/Contents/Info.plist")"
  [ "$name" = "$main" ] && continue
  /usr/bin/codesign --force --sign "$IDENT" --identifier "$BID.$name" "$f"
done

# The app itself. Ad-hoc: pin the designated requirement to the identifier (the default would be the
# cdhash, which changes every build and orphans every TCC grant).
if [ "$IDENT" = "-" ]; then
  /usr/bin/codesign --force --sign - --identifier "$BID" --entitlements "$(dirname "$0")/../crates/lowk-app/Entitlements.plist" -r="designated => identifier \"$BID\"" "$APP"
else
  /usr/bin/codesign --force --sign "$IDENT" --identifier "$BID" --options runtime \
    --entitlements "$(dirname "$0")/../crates/lowk-app/Entitlements.plist" "$APP"
fi

/usr/bin/codesign --verify --strict --verbose=2 "$APP"
got="$(/usr/bin/codesign -dv "$APP" 2>&1 | sed -n 's/^Identifier=//p')"
[ "$got" = "$BID" ] || { echo "signing Identifier '$got' != bundle id '$BID'" >&2; exit 1; }
echo "signed $APP: Identifier=$got, identity=$([ "$IDENT" = "-" ] && echo ad-hoc || echo "$IDENT")"
