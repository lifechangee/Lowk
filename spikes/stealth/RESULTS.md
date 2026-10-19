# Stealth spike results (macOS 27.0.1, MacBook Air M4)

STATUS: **Permission granted; all scenarios run.** Summary: with `sharingType = .none` the overlay was hidden from all four of our capture methods
(SCK screenshot, SCK stream, legacy CGWindowListCreateImage, `screencapture`) in every scenario, while the main window stayed visible. A negative control
(exclusion off) proves the probe can see the overlay. This contradicts the "SCK ignores sharingType on macOS 15+" reports, at least for
display-wide SCK filters with no `excludingWindows` on macOS 27.0.1. It says nothing yet about third-party apps (Zoom/Teams/OBS/QuickTime): run the manual checklist.

## What is built and verified
- `app/` Tauri 2 spike. Builds and runs on macOS (`cargo build`, ~1 min). Windows: `cargo check --target x86_64-pc-windows-msvc` passes
  (needs a dummy `llvm-rc` on PATH on a Mac host because of the .ico resource step; irrelevant on real Windows).
- Overlay (magenta `OVERLAY-MARKER`, 360x160, transparent, level 1000, collectionBehavior CanJoinAllSpaces|Stationary|IgnoresCycle|FullScreenAuxiliary,
  Accessory activation policy) and main (green `MAIN-MARKER`, normal window).
- Overlay NSWindow class-swapped to NSPanel via `object_setClass` (tao class was `NSKVONotifying_TaoWindow`) + NonactivatingPanel mask: runs without crash (focus-steal not yet tested).
- Rust owns `sharingType = .none`: applied at setup/show and on Moved/Resized/Focused/ScaleFactor events,
  NSWorkspace activeSpaceDidChange, and a 1 s watchdog with readback. Log (`run.log`) shows `before=0 after=0 ok=true` on every check, i.e.
  the *flag* sticks (this says nothing about whether capturers honor it).
- Hotkeys: Cmd+Shift+H panic-toggle overlay, Cmd+Shift+M toggle main. Control file `spike.ctl` drives scenarios (main-hide/show, overlay-move x y, overlay-hide/show, overlay-toggle-exclusion on|off [negative control], quit).
- Windows path `app/src-tauri/src/win.rs`: SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE) with result check + GetWindowDisplayAffinity readback, WS_EX_TOOLWINDOW|NOACTIVATE, TOPMOST. Type-checks only; untested at runtime.
- `probe/probe.swift`: (a) SCScreenshotManager, (b) SCStream frame, (c) CGWindowListCreateImage via dlsym, (d) screencapture CLI; counts magenta/green pixels, saves PNGs + JSON.

## How to run
```
cd spikes/stealth
./run-scenarios.sh        # builds, launches app, runs scenarios 1-8, writes results/*.png, results/*.json, run.log
```
Scenario 5 (exclusion off) is a negative control: the probe MUST see the overlay there, or detection is broken.

## Results table
Cell = overlay marker / main marker. H = hidden, V = visible (pixel count > 500). Magenta residue of ~45 px is blend noise.
| Scenario | a SCK screenshot | b SCK stream | c CGWindowList (dlsym, still works) | d screencapture CLI |
|---|---|---|---|---|
| 1 overlay alone (main hidden) | H / H | H / H | H / H | H / H |
| 2 overlay + main | H / V | H / V | H / V | H / V |
| 3 after main hide/show | H / V | H / V | H / V | H / V |
| 4 after overlay move | H / V | H / V | H / V | H / V |
| 5 NEGATIVE CONTROL (exclusion off) | **V** / V | **V** / V | **V** / V | **V** / V |
| 6 exclusion back on | H / V | H / V | H / V | H / V |
| 7 overlay panic-hidden | H / V | H / V | H / V | H / V |
| 8 over a fullscreen Space (main not visible there) | H | H | H | H |
| 8b NEGATIVE CONTROL in fullscreen Space (exclusion off) | **V** | **V** | **V** | **V** |

Notes: 8b proves the overlay does sit above a fullscreen app's Space (CanJoinAllSpaces|FullScreenAuxiliary, level 1000), so 8 is a real "hidden", not "absent".
CGWindowList `kCGWindowSharingState` is 0 for the overlay and 1 for main (useful as a cheap self-check). Automated Space switching via keyboard was not done;
only the fullscreen-Space case (a helper blue fullscreen window, `probe fsdummy`).
Not covered: SCK filters built per-window or with `including` apps, SCContentSharingPicker, Zoom/Teams/OBS/QuickTime/Chrome (see checklist), multi-display.

## Focus-steal test (`focus-test.sh`)
TextEdit frontmost; overlay hide, show, two moves, and a synthetic CGEvent click on the overlay: the frontmost app stayed TextEdit throughout
(checked with `lsappinfo front`). Caveat: synthetic click may have been dropped without Accessibility permission, so "real click does not steal focus" still needs a
manual check (click overlay while typing in TextEdit; typing must continue). Overlay never appears in Dock/Cmd+Tab (Accessory policy).

## Harness/spike bugs found and fixed
- Calling AppKit (`orderFrontRegardless`, style changes) from the watchdog thread hung that thread: panic hide -> show silently broke everything after it
  (overlay then did not appear over fullscreen Spaces, watchdog stopped). Fix: all show/after_show work runs via `run_on_main_thread`. Real-app rule: every NSWindow call on the main thread.
- tao `show()` can reset level/collectionBehavior/style: `after_show` re-asserts them every time.
- A freshly shown Accessory-app main window was behind other apps; the harness now uses `main-focus`.
- Stale `spike.ctl` caused instant quit on next launch; script now deletes it.
- `object_setClass` to NSPanel worked (original class `NSKVONotifying_TaoWindow`); risky with KVO, prefer tauri-nspanel in the real app.

## Recommendations
1. Use `sharingType = .none` + the Rust-owned re-apply/watchdog/readback; on this macOS 27.0.1 it works against SCK and legacy capture (flag never drifted: 0 failed checks in run.log).
2. Keep the panic hotkey; never touch AppKit off the main thread; re-assert level/collectionBehavior after every show.
3. Ship a self-test button (capture own display via SCK with overlay marker, as the probe does) to show green/red in settings; needs Screen Recording TCC.
4. Re-run this table per macOS update and per Chrome stable; treat third-party capturers (OBS/QuickTime/Zoom) as unverified until the manual checklist is done.
5. Windows: run `win.rs` on the real PC, check log lines for `SetWindowDisplayAffinity=Ok(())` and affinity=0x11.

## Manual checklist (Zoom / Meet / Teams / Discord / OBS / QuickTime)
For each: start share of entire screen (and again of a single window), with overlay visible and main open; confirm in the remote/preview view that
magenta is absent and green is present; then move overlay, toggle main, switch Spaces, enter fullscreen app, press Cmd+Shift+H. Repeat with the app's
"window filtering"/"advanced capture" modes where they exist (Zoom). Record app + version. Chrome/Meet: re-test each Chrome stable (SCK capturer rollout).
OBS: test both "Display Capture" and "macOS Screen Capture" sources. QuickTime: File > New Screen Recording.

## Real-app tests
STATUS: **NOT RUN (blocked).** Computer-use access was granted for Zoom, Discord, QuickTime Player (Chrome read-only tier), and full-screen control was approved,
but every display-scope action (screenshot/key) was aborted with "user interrupt" (the user was apparently using the machine), so no real-app capture was performed.
Note: the spike is a bare binary (`target/debug/stealth-spike`), not an .app, so it cannot be named in request_access (not needed; launch it via shell).
Chrome/Meet additionally needs the claude-in-chrome MCP (Chrome is only "read" tier in computer-use).

| App | Capture path | Overlay visible? | Main visible? | Evidence | Notes |
|---|---|---|---|---|---|
| QuickTime | New Screen Recording | not tested | not tested | - | menu item disabled while app in background; needs frontmost + full control |
| Zoom | share entire screen + local recording | not tested | not tested | - | |
| Discord | DM call to second-test-account + screen share | not tested | not tested | - | |
| Google Meet (Chrome) | present entire screen | not tested | not tested | - | |

Re-run attempt (user away): display-scope computer-use calls still aborted with "user interrupt" even after releasing/re-requesting full control (harness-side, not a user action). Real-app tests remain NOT RUN; table above unchanged.

### Real-app tests, run 3 (orchestrator, 2026-10-19, macOS 27.0.1)

| App | Capture path | Overlay visible? | Main visible? | Evidence | Notes |
|---|---|---|---|---|---|
| Discord (desktop) | Window picker ("Приложения") | **No** — overlay not offered as a shareable window | Yes — "Lowk Spike MAIN" listed with green thumbnail | evidence/discord/window-picker.jpg | |
| Discord (desktop) | Live "Весь экран" stream in a DM call, own stream preview | **No** — overlay region (x100,y150 360×160 pt, layer 1000, on-screen, kCGWindowSharingState=0) shows Discord sidebar underneath | n/a (main was behind other windows) | evidence/discord/stream-preview.jpg | Preview pauses when Discord is in background; checked with Discord frontmost |
| Zoom | — | not tested | — | — | Zoom not signed in; hosting a meeting needs login (not entered by agent) |
| Google Meet (Chrome) | — | not tested | — | — | Needs claude-in-chrome or manual test |
| QuickTime | — | not tested | — | — | Same SCK path as our `screencapture`/SCK probes, which were clean |

Window server confirms per-window state: overlay `kCGWindowSharingState=0` (none), main `=1` (read-only/shareable) — i.e. exclusion is per window, the predecessor's two-window bug is not inherent to macOS.
