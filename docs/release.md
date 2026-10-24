# Test builds (Windows and macOS) from GitHub Actions

Workflow: `.github/workflows/release-build.yml` (`release-build`). It runs on demand (nightly to be added once stable).
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
- Tauri updater signing keys are not used by these builds.

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
