# Leak audit, 2026-10-22

Scope: public repo lifechangee/Lowk, all remote branches (main, ci/release-build), no tags, full history (137 commits, 2865 objects), Actions runs.

## Findings
1. No secrets found. History diff and every git object were scanned for provider token patterns (OpenRouter, Anthropic, OpenAI, GitHub, AWS), private key headers, and the values of the owner's local test keys (Soniox, Deepgram, OpenRouter). Zero hits. No .env, .p12/.pem/.key, keychain, or code-signing cert files ever tracked.
2. No personal data found: owner's real email, local username, local paths, phone-like numbers. All 137 commits use the GitHub noreply identity. The word "Discord" appears only as an app name in QA docs. No audio files; tracked PNGs are app UI screenshots (design audit, QA), not desktop captures (spot-checked).
3. Actions: 60 recent runs (24 success, 11 failure, 24 cancelled). Sampled logs (including failures): no key values, no owner paths (only runner paths), no owner email. No artifacts, no repo secrets.
4. Old pre-rewrite commits: every head SHA referenced by Actions runs (58) exists in the current history, so no orphaned pre-rewrite commits are linked from runs. No PRs, no forks. Old SHAs cannot be enumerated from the API, so a direct-SHA lookup by anyone who already knows an old SHA may still work; GitHub Support can purge cached views.
5. Repo settings (weak): secret scanning disabled, push protection disabled, Dependabot security updates disabled, main not protected, no rulesets. Workflow default token permission is read-only (good).

## Recommended actions
- Enable secret scanning + push protection, Dependabot alerts/security updates.
- Protect main (block force-push and deletion; require CI).
- Optionally ask GitHub Support to purge cached pre-rewrite commits.
- Rotate nothing: no key exposure detected.
