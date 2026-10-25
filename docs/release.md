# Releasing Lowk

Releases are cut by pushing a version tag. Test builds without a tag: see "Test builds" below.

## One-time setup (maintainer)
1. Generate the updater key pair locally (never commit it, never paste it into chat):
   `cargo tauri signer generate -w ~/.tauri/lowk.key` and keep the passphrase in a password manager. Back the key up offline.
2. Put the PUBLIC key (the contents of `~/.tauri/lowk.key.pub`) into `crates/lowk-app/tauri.conf.json` -> `plugins.updater.pubkey`.
   While it is empty the updater is disabled.
3. In GitHub -> Settings -> Secrets and variables -> Actions add `TAURI_SIGNING_PRIVATE_KEY` (the content of `lowk.key`) and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

## Cutting a release
1. `just bump 0.1.0-rc.1` (version source of truth: `[workspace.package]` in `Cargo.toml`; the script also updates
   `ui/package.json`, the worker crates and the lockfiles; `tauri.conf.json` inherits). `just lint` fails if they disagree.
2. Add a `## 0.1.0` section to `CHANGELOG.md` (English).
3. Commit, then tag and push: `git tag v0.1.0-rc.1 && git push origin v0.1.0-rc.1`.
4. The `release` workflow builds macOS (dmg, app zip, updater tar.gz) and Windows (NSIS x64, Vulkan with automatic CPU
   fallback), signs the updater artifacts, writes `latest.json` and `SHA256SUMS.txt`, and creates a DRAFT release
   (`-rc.N` tags are marked pre-release; the updater ignores pre-releases and drafts). The job summary shows installer sizes against the
   targets (dmg <= 25 MB, NSIS <= 20 MB; warnings only).
5. Check the draft: assets, checksums, notes. Fill in the Russian summary placeholder at the end of the body.
6. Walk through a clean-machine install of the rc. When it is good, bump to `0.1.0`, tag `v0.1.0`, check the draft again and press Publish.
   Only a published, non-pre-release release becomes `releases/latest/download/latest.json`, which is the updater endpoint.

A non-rc tag fails if the signing secrets are missing; an rc tag then builds without signed updater artifacts (and without `latest.json`) and warns.

## Where things live
- Bundle identifier: `identifier` in `crates/lowk-app/tauri.conf.json` (the only place to edit for the build). Other mentions that must
  follow a change: the keychain service constant in `crates/lowk-platform/src/secrets.rs` (existing saved keys would need migrating) and the
  macOS signing docs. `tools/sign-bundle.sh` reads the id from the built bundle.
- Updater public key and endpoint: `plugins.updater` in `crates/lowk-app/tauri.conf.json`.
- Minimum macOS (14.4): `bundle.macOS.minimumSystemVersion` in `crates/lowk-app/tauri.bundle.conf.json`.
- Release tooling: `tools/release/` (`bump.sh`, `check-versions.sh`, `check-no-keys.sh`, `make-latest-json.sh`, `size-report.sh`, `changelog-section.sh`).
- Guards: `*.key` is git-ignored and `tools/release/check-no-keys.sh` (part of `just lint`) fails if key material is tracked.

# Test builds (Windows and macOS) from GitHub Actions

Workflow: `.github/workflows/release-build.yml` (`release-build`). It runs on demand (nightly to be added once stable) and is also called by `release.yml`.
Each run builds the full bundled app, runs `cargo test --workspace` on that OS and uploads installers as artifacts
(kept 14 days). Models are never bundled; they are downloaded on first use.

## How to get a test build
1. GitHub -> repo -> Actions -> `release-build` -> pick the latest green run (or `Run workflow`).
2. Scroll to **Artifacts** and download:
   - `lowk-windows-x64-vulkan` : NSIS `.exe` installer (per-user, no admin rights).
   - `lowk-macos-arm64` : `Lowk-<ver>-macos-arm64.dmg` and `.app.zip`.
3. CLI: `gh run download <run-id> -n lowk-windows-x64-vulkan`.

Manual run options: `windows_gpu` = `vulkan` (default) | `cpu` | `cuda`; `platforms` = `both` | `windows` | `macos`.

## What is inside
| | Windows x64 | macOS arm64 |
|---|---|---|
| App | NSIS installer, WebView2 bootstrapper | `Lowk.app`, AEC (WebRTC) on |
| `lowk-stt-worker` | Vulkan by default (works on any GPU with a Vulkan driver, falls back to CPU if none); `cpu` and `cuda` via the run option | Metal |
| `lowk-embed-worker` (llama-server) | CPU | Metal |
| Models | not bundled | not bundled |

CUDA needs the CUDA toolkit on the runner and NVIDIA GPUs on the user's side, so it is opt-in and untested in CI.

## What is unsigned
- Windows: no Authenticode signature. SmartScreen shows a blue "Windows protected your PC / Unknown publisher"
  dialog: click **More info** then **Run anyway**. Edge/Chrome may also say "isn't commonly downloaded": Keep.
- macOS: ad-hoc signed (designated requirement pinned to the bundle id `dev.lowk.app`, same as `just build`), not
  notarized. A downloaded `.dmg`/`.zip` carries the quarantine flag, so Gatekeeper says "Lowk can't be opened because
  Apple cannot check it for malicious software". Fix: right-click the app -> **Open** -> Open, or
  `xattr -dr com.apple.quarantine /Applications/Lowk.app`. (On macOS 15+: System Settings -> Privacy & Security ->
  "Open Anyway".)
- Manual `release-build` runs do not sign updater artifacts; the tag-triggered `release` workflow does.

## Notes
- These are test builds, not releases. Release builds of the macOS app go through `just build` + `just qa-bundle`
  (see [testing.md](testing.md)); Windows runtime is unverified until real hardware.
- Scripts: `tools/release/stage-sidecars.sh` (builds and stages both sidecars, writes `tauri.release.conf.json`,
  generated, not committed), `tools/release/make-dmg.sh` (dmg + zip from the signed app).
- Caches: rust-cache, pnpm store, llama.cpp build dir; a warm run is much faster than the first.

## Known state of the Windows Vulkan variant
`windows_gpu=cpu` is verified green (run 39751106683). `vulkan` got through cmake (SPIRV-Headers from vcpkg, run
39755378234) but failed at link with LNK1181 `vulkan-1.lib`; fixed afterwards by passing
`-L native=$VULKAN_SDK/Lib` via RUSTFLAGS in the sidecar step. That fix has NOT been verified by a run yet:
if `vulkan` still fails, use `cpu`.
