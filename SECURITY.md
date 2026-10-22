# Security policy

Lowk is pre-alpha software. Only the latest commit on `main` is supported.

## Reporting a vulnerability

Please do **not** open a public issue. Use GitHub's private reporting: on the repository page open the **Security** tab and choose **Report a vulnerability**. If that option is not available, open a public issue that says only that you have a security report and want a private channel, with no technical details, and a maintainer will reach out.

Include: what you found, the affected version or commit, your OS, steps to reproduce, and the impact you see. Please give us reasonable time to fix the problem before disclosing it; we will acknowledge a report as soon as a maintainer can and keep you updated. This is a volunteer project and we cannot promise a fixed timeline.

## Scope

In scope examples: leaking API keys or transcripts, sending data to the cloud when a folder is marked local only or without a user action, the overlay appearing in screen capture contrary to its documented behaviour, path traversal or overwrite on document import and vault import, unsafe handling of files in document conversion, the local QA channel being reachable in a release build.

Known and documented limits are not vulnerabilities: vault files are unencrypted Markdown (encryption is planned), the stealth overlay depends on the OS and the sharing app and is not guaranteed, and the process is visible in the task manager.

## What Lowk stores and sends

API keys are kept in the OS keychain. There is no telemetry. For which data goes to cloud providers and when, see the Privacy section of the [README](README.md).
