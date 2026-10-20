# M0 status (2026-10-20, MacBook Air M4, macOS 27.0.1)

All M0 packages are integrated on `main`. `just ci` is green (fmt, clippy `-D warnings`, `check-deps`, `pnpm check`, `cargo deny`, workspace tests, ui tests/build/size). `cargo check --workspace --target x86_64-pc-windows-msvc` passes (dummy `llvm-rc`, `ml64.exe`, `lib.exe` on PATH are not needed any more except `llvm-rc`).

## Exit criteria

| # | Criterion | Result |
|---|---|---|
| 1 | Stealth probe table, macOS | PASS, 18/18 scenarios against the real app (table below). Fullscreen Space not automated: unverified. Windows: unverified (no runtime). |
| 2 | Typing into the overlay without focus loss | NOT verified here (needs a human typing session and a frontmost app to observe). Mode machine, `canBecomeKey` allow-list and `overlay_input_submit` are wired and unit-tested; interactive mode toggles natively (probe scenario 5). |
| 3 | Magnet move via hold bind / side button | Magnet verified with a scripted cursor (scenario 6). The live hold hooks are NOT verified: Input Monitoring / Accessibility are not granted to the dev binary, so `MoveHold` reports `Degraded(InputMonitoring)` (`binds` probe output) and the combo binds report `Active`. Not bypassed. |
| 4 | Vault create/open, folder/call CRUD, golden `.md` | Vault opens/creates, demo call recorded as `.md` in `Unsorted/Calls`, library renders it in the main window (real data). Golden round-trip tests pass in `lowk-store`. Folder/call mutation commands are implemented and covered by `lowk-store` tests; clicking through them in the real webview was not automated (no UI driver for the unbundled binary). |
| 5 | RU/EN live switch | PASS: `ui.language` patch re-renders the open main window and the overlay via `settings://changed` (screenshots checked, RU -> EN). |
| 6 | Demo transcript in overlay + tone sampler | PASS: demo feed renders in the overlay (subtitle line, answer area, «Мой ответ»); the sampler returned a real sample (`luma 0.0097`, `busy 0.12`) through ScreenCaptureKit with Screen Recording granted. |
| 7 | Settings persist; keys in keychain | PASS: `settings.toml` rewritten atomically (`language`, `opacity` seen on disk). Keychain: dummy key set, presence true, deleted, presence false (`secret-test`). |

## Stealth probe against the real app (`just stealth-probe-app`)

Overlay page = solid magenta, main page = solid green; four capture paths per scenario (SCK screenshot, SCK stream, CGWindowList, `screencapture`).

| Scenario | Expectation | Result |
|---|---|---|
| 0 main only | overlay hidden, main visible | PASS |
| 1 both windows open | overlay hidden, main visible | PASS |
| 2 main hide/show x5 | same | PASS |
| 3a main destroyed | both absent | PASS |
| 3b/3c main re-created (twice) | overlay hidden, main visible | PASS |
| 4 overlay moved | same | PASS |
| 5 interactive mode | same | PASS |
| 6 after magnet drag | same | PASS |
| 7 activation-policy flip | same | PASS |
| 8 NEGATIVE CONTROL (exclusion off) | overlay visible | PASS (the probe sees the overlay when it should) |
| 9 exclusion back on | overlay hidden | PASS |
| 10 panic | overlay hidden | PASS |
| 11 panic restore | overlay hidden (apply -> verify -> show) | PASS |
| 12 overlay hidden | same | PASS |
| 13 hide-main on | both hidden | PASS |
| 14 hide-main off | overlay hidden, main visible | PASS |
| 15 main hide/show x25 stress | overlay hidden, main visible | PASS |

## Unverified / remaining

- Windows runtime (all of it: `WDA_EXCLUDEFROMCAPTURE`, hooks, GDI sampler is a stub).
- Live hooks: needs Input Monitoring (hold, modifier-only, mouse buttons, record mode) and Accessibility (swallowing) for the app binary; plain combos were registered without a permission.
- Typing in the overlay on a human session; IME candidate window is unprotected by design.
- Fullscreen Space and third-party capturers (Zoom/Teams/Meet/OBS/QuickTime).
- Click-through of the main-window CRUD and onboarding in the real webview (backend paths and rendering are verified).
- Header drag in interactive mode (pointer-drag commands exist; not exercised).
- Light-background legibility at 90% transparency was changed by CSS token clamp and unit-tested, not eyeballed on a bright wallpaper.
- Native folder picker, updater, documents/search/summary (deferred by design, see DECISIONS-M0 46).
- Demo date in titles/`started` is UTC (local offset source is post-M0).
