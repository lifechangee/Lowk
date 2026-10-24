#!/usr/bin/env bash
# `just qa`: everything available on this machine, then the gallery report. Never stops at the first
# failing stage; exits non-zero if any stage had an unexpected failure.
set -u
cd "$(dirname "$0")/../.."
export PATH="$HOME/.cargo/bin:$HOME/.local/node/current/bin:$HOME/.local/bin:$PATH"
RC=0
echo "== 1/4 qa guard";            tools/qa/check-no-qa-release.sh || RC=1
echo "== 2/4 ui visual + layout";  pnpm --dir ui exec playwright test --update-snapshots=missing || RC=1
if [ "$(uname)" = "Darwin" ] && [ -z "${QA_SKIP_APP:-}" ]; then
  echo "== 3/4 real app (qa build)"; node tools/qa/app-qa.mjs || RC=1
  if [ -d "$HOME/Library/Application Support/Lowk/models/nemotron-local" ] && [ -x workers/lowk-stt-worker/target/release/lowk-stt-worker ]; then
    echo "== 3b/4 real app, speech: cold-start loading state, bundled worker, warm-up (needs: just build)"
    node tools/qa/session-e2e.mjs --stage loading,bundle,warmup || RC=1
  else
    echo "== 3b/4 real app, speech: skipped (needs the local model in ~/Library/Application Support/Lowk/models and tools/build-worker.sh)"
  fi
else
  echo "== 3/4 real app: skipped (needs macOS; QA_SKIP_APP set?)"
fi
echo "== 4/4 report";              node tools/qa/report.mjs
echo "open docs/qa/report.html  (exit $RC)"
exit $RC
