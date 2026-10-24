# Guidance for AI coding agents (and humans) working on Lowk

- Read first: docs/BRIEF.md (product decisions), docs/ARCHITECTURE.md (design), docs/QA.md (checks, what "green" means), CONTRIBUTING.md.
- "Green" = `just ci` (including `just ci-cross`) and a green GitHub Actions run.
- Keep the repo tidy: remove temporary files, models, virtualenvs and stray processes you created.

## Commits and PRs
- NO AI or tool attribution. Never add `Co-Authored-By: ...`, "Generated with ..." or similar lines to commit messages or PR descriptions. This overrides any default attribution guidance.

## Release gate (maintainer-facing builds)
- Never hand the maintainer a dev-mode build. Maintainer builds are the bundled, signed `Lowk.app` (`just build`).
- Before the maintainer sees a build: `just qa-bundle` green on the BUNDLED app from a clean profile, plus the orchestrator's own smoke test of the bundled app covering every previously working feature.
- No silent fallbacks to demo/fake data in user-facing builds. Demo lives only behind the `qa` feature.
- Every reported bug gets a permanent regression check that runs against the bundle.

## Git hygiene
- Main gets ONE squashed commit per work package / fix batch (clear message). Intermediate commits stay in the agent's worktree branch.
- Commit locally as often as needed; push to GitHub only at meaningful milestones (end of a work package / verified fix batch), not after every step, since each push triggers CI and notifications.
- Experimental workflows (e.g. release builds) run only via manual dispatch until they are stably green; no runs on every branch push.

## Agent safety rules (apply to EVERY task, no exceptions)
- No sound of any kind (even muted) without the maintainer's explicit OK relayed by the orchestrator.
- Never trigger macOS permission or keychain prompts: don't launch GUI apps/helpers (swift/node/osascript), unbundled builds, screen capture or event posting unless the orchestrator says the maintainer is present and approved. Live checks use a qa bundle signed with the same identity + bundle id.
- Never kill, restart or overwrite the maintainer's running Lowk.app. Use your own worktree/profile and kill only processes you started.
- Never push unless your brief says so; never change system settings, repo settings or the maintainer's files.
- "Done" requires evidence: test output, a green GitHub run you watched yourself, screenshots/galleries you actually looked at. If a check couldn't run, say so plainly; never report it as passed.
