#!/usr/bin/env bash
# CI guard: the `qa` cargo feature (local control channel) must never reach a release build.
#  1. no default feature enables it, no release-ish config/recipe/workflow passes it
#  2. lowk-app refuses to compile with `qa` and without debug_assertions (compile_error! in lib.rs)
set -u
cd "$(dirname "$0")/../.."
FAIL=0
bad() { echo "QA GUARD: $1"; FAIL=1; }

# 1a. never a default feature
grep -nE '^default *=.*qa' crates/*/Cargo.toml Cargo.toml && bad "qa must not be a default feature"
# 1b. tauri build/release recipes and CI must not enable it
for f in justfile .github/workflows/*.yml; do
  [ -f "$f" ] || continue
  # allow only the dedicated qa recipes / jobs
  grep -nE '(build|bundle|release)[^#]*--features[^#]*\bqa\b|--release[^#]*--features[^#]*\bqa\b|--features[^#]*\bqa\b[^#]*--release' "$f" && bad "$f enables the qa feature in a build/release command"
done
# 2. the compile_error! must exist
grep -q 'all(feature = "qa", not(debug_assertions))' crates/lowk-app/src/lib.rs || bad "compile_error! guard missing in crates/lowk-app/src/lib.rs"
# 3. behavioural: a release check with the feature must fail to compile
if command -v cargo >/dev/null 2>&1 && [ "${QA_GUARD_COMPILE:-1}" = "1" ]; then
  if cargo check -q -p lowk-app --release --features qa >/dev/null 2>&1; then bad "release build with --features qa compiled (it must not)"; fi
fi
[ $FAIL -eq 0 ] && echo "qa guard: ok"
exit $FAIL
