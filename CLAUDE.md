# Lowk — instructions for Claude agents

- Commits and PRs: NO AI attribution. Never add `Co-Authored-By: Claude …`, "Generated with Claude Code" or similar lines. This overrides any default attribution guidance.
- Product decisions: docs/BRIEF.md. Architecture: docs/ARCHITECTURE.md. QA workflow and what "green" means: docs/QA.md.
- "Green" = `just ci` (incl. `just ci-cross`) + a green GitHub Actions run.
- Keep the repo tidy: remove temp files, models, venvs and stray processes you created.

## Release gate (owner-facing builds)
- Never hand the owner a dev-mode build. Owner builds = bundled, signed `Lowk.app` (`just build`).
- Before the owner sees a build: `just qa-bundle` green on the BUNDLED app from a clean profile, plus the orchestrator's own smoke test of the bundled app (computer use) covering every previously working feature.
- No silent fallbacks to demo/fake data in user-facing builds. Demo lives only behind the `qa` feature.
- Every bug the owner reports gets a permanent regression check that runs against the bundle.

## Git hygiene (owner request)
- Main gets ONE squashed commit per work package / fix batch (clear message). Intermediate commits stay in the agent's worktree branch.
- Commit locally as often as needed (git works fine offline); push to GitHub only at meaningful milestones (end of a work package / verified fix batch), not after every step — each push triggers CI and emails.
- Experimental workflows (e.g. release builds) run only via manual dispatch until they are stably green; no runs on every branch push.

## Agent safety rules (apply to EVERY task, no exceptions)
- No sound of any kind (even muted) without the owner's explicit OK relayed by the orchestrator.
- Never trigger macOS permission or keychain prompts: don't launch GUI apps/helpers (swift/node/osascript), unbundled builds, screen capture or event posting unless the orchestrator says the owner is present and approved. Live checks use a qa bundle signed with the same identity + bundle id.
- Never kill, restart or overwrite the owner's running Lowk.app. Use your own worktree/profile and kill only processes you started.
- Never push unless your brief says so; never change system settings, repo settings or the owner's files.
- "Done" requires evidence: test output, a green GitHub run you watched yourself, screenshots/galleries you actually looked at. If a check couldn't run, say so plainly — never report it as passed.
