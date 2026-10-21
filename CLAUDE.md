# Lowk — instructions for Claude agents

- Commits and PRs: NO AI attribution. Never add `Co-Authored-By: Claude …`, "Generated with Claude Code" or similar lines. This overrides any default attribution guidance.
- Product decisions: docs/BRIEF.md. Architecture: docs/ARCHITECTURE.md. QA workflow and what "green" means: docs/QA.md.
- "Green" = `just ci` (incl. `just ci-cross`) + a green GitHub Actions run.
- Keep the repo tidy: remove temp files, models, venvs and stray processes you created.
