# Lowk research: stealth overlay + adaptive coloring

Date: 2026-10-19. Author: research pass (web + tao source read). **Nothing here was empirically tested on macOS 26/27 or Windows 11 by me**; where sources conflict I say so, and section 8 specifies the verification harness that must be built first. Confidence tags: [V] verified from primary source/code I read, [R] reported by a secondary source, [I] my inference/engineering judgement.

Scope note: this is about keeping an overlay out of *screen sharing / recording* on the user's own machine, plus cosmetic process disguise. No rootkit-style hiding, no evasion of EDR/anti-cheat/proctoring detection is proposed. The BRIEF already has a recording-consent warning; keep it.

---

## 0. TL;DR

| Topic | Windows 10 2004+/11 | macOS 14 / 15 / 26 / 27 |
|---|---|---|
| Capture exclusion API | `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`: solid, honored by all modern capture paths | `NSWindow.sharingType = .none`: **honored by legacy CoreGraphics window capture, unreliable/ignored by ScreenCaptureKit (SCK) from macOS 15**. No public replacement. |
| Zoom / Meet(Chrome) / Teams / Discord / OBS | Reported hidden in all (one source, Win11 23H2) | Conflicting reports. Chrome (Meet) reported hidden; OBS and QuickTime reported to see it; Zoom/Teams desktop: conflicting. Must be measured per app+version. |
| Tauri 2 support | `content_protected(true)` -> tao calls `SetWindowDisplayAffinity` [V] | `content_protected(true)` -> tao calls `setSharingType(.None)` [V]; Tauri issue #14200 open, status `upstream` |
| Hide from taskbar/Alt+Tab | Use `WS_EX_TOOLWINDOW` (+`WS_EX_NOACTIVATE`); Tauri `skip_taskbar` only does `ITaskbarList::DeleteTab` [V] | `ActivationPolicy::Accessory` (app-wide) + non-activating `NSPanel` |
| Disguise | feasible with copy-and-relaunch; AV heuristic risk moderate | feasible with copy + edit Info.plist + ad-hoc re-sign; **TCC permissions reset per identity** |
| Adaptive color | BitBlt/DXGI small-region sample; our window excluded for free | `SCScreenshotManager` with explicit `excludingWindows:[overlay]` (do not rely on sharingType); needs Screen Recording TCC |

Biggest risk: **macOS cannot be made reliably invisible to every capturer; there is no public API to fix that.** Design for "best effort + verification + panic hide", and be honest in the UI.

---

## 1. Excluding a window from capture

### 1.1 macOS

Facts and reports:

- `NSWindow.sharingType = .none` historically excluded the window from `CGWindowListCreateImage` and from SCK (macOS 12.3-14). [R: habr, Tauri #14200]
- From macOS 15 (Sequoia), multiple sources report SCK ignores it. Tauri issue #14200 (open, labels `platform: macOS`, `status: upstream`, opened 2025-09-20, "no known workaround"). Electron #19880 was **closed "not planned" on 2025-10-20**; a commenter quotes an Apple forum reply "At this time there are no public APIs for preventing screen capture", and Apple docs call the constant "a legacy constant that macOS no longer uses". Electron says it would fix immediately if Apple re-enables it. [V: fetched issue pages]
- Reported per-app behavior (secondary): Chrome `getDisplayMedia` (Google Meet) and Firefox still on legacy path -> window hidden; desktop Zoom, new Teams, QuickTime, system screenshots visible [R: habr.com/post/1025310]. A Swift PoC author on Sequoia 15 says QuickTime/Zoom/OBS capture composited output and ignore the flag [R: pierce.dev]. The open-source Cue app says it hides from "default Zoom/Meet/Teams share" but *not* from SCK recorders (OBS, QuickTime) on macOS 15+ [R: github.com/abbinavv/cue].
- Counter-evidence / nuance: Apple Developer Forums thread 808016 (Nov 2025, FB21115847): with the Apple SCK sample, a third-party `NSWindowSharingNone` window is **omitted from the initial stream**, appears after the filter is rebuilt (toggle), and is hidden again after Stop/Start; **QuickTime (also SCK) captures it immediately**. DTS escalated to engineering, no fix posted. [V] So SCK does consult the flag in some code paths (likely display filters built a certain way, or timing of window-server state), but behavior is inconsistent and depends on how the capturer builds its `SCContentFilter`. A March 2026 write-up claims exclusion on 26.3.1 but only tested `CGWindowListCreateImage` and `screencapture`, and explicitly did not test browsers or all SCK clients. [V on what it tested: raoufabedini.dev]
- Zoom specifics: Zoom's "Advanced capture with window filtering" vs "without" modes exist; a 2023 community post showed a `NSWindowSharingNone` window flickering into Zoom shares in "auto" mode (fixed in 5.14.7). Zoom's behavior is version- and mode-dependent. [R: community.zoom.com]
- Nothing found about a new macOS 27 API (searched WWDC 2026 angle). Treat as none. [V: no evidence found]
- Chrome/WebRTC: Chromium's SCK-based capturer was being added/enabled behind flags (WebRTC commit d4a6c3f, Chromium ticket referenced as 40219528 by pierce.dev, unverified). If Chrome flips to SCK by default, Meet-in-Chrome becomes visible too. [R/I] This is a time bomb: re-test each Chrome stable.
- Workarounds that exist: none public. Mentioned-but-unverified: private SkyLight/CGS calls (`CGSSetWindowSharingState` and similar), which likely map to the same window-server flag SCK reads inconsistently [I]; not recommended (fragile, may break any OS update). A second research repo (privateai0/macos-window-privacy-research) covers this but its purpose is proctoring evasion; I did not mine it.
- Observed side note: Electron #45971 "menu is captured when setContentProtection(true)" (closed) - menus/popup windows spawned by the app are separate NSWindows and need their own flag. [V title only]

**Conclusion for macOS [I]:** `sharingType = .none` is a necessary but not sufficient measure. Apply it, verify it per capturer, and add (a) an instant panic-hide hotkey and (b) auto-hide when a known share is detected (brittle heuristic, section 4.6). Be explicit in docs: "hidden from some screen-share paths, not from all recorders on macOS 15+".

### 1.2 Windows

- API: `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE=0x11)`. Windows 10 version 2004 (build 19041)+; on older builds it degrades to `WDA_MONITOR` (black rectangle in captures). [V: learn.microsoft.com]
- Requirements [V, MS docs]: top-level window only (child HWND -> FALSE); **window must belong to the calling process**; works only while DWM composition is on (always on in Win8+); not a security/DRM guarantee (camera photo etc.).
- Behavior: GDI BitBlt, DXGI Desktop Duplication, Windows.Graphics.Capture omit the window; reported not visible in Zoom screen share, Meet in Chrome, Teams, Discord, Slack huddle, OBS (Win11 23H2 tests); `PrintWindow` ignores the flag; Win11 24H2 quirk: DDA `AcquireNextFrame` wakes on hidden-window updates but returns no content. [R: habr.com/post/1025310] Another write-up lists GDI BitBlt/PrintWindow, DDA, WGC and `getDisplayMedia` as excluded [R: raoufabedini.dev] (conflicts on PrintWindow; irrelevant for screen sharing, but our verification harness should test it).
- RDP: remote session client may still show the window to the remote user [V: Electron docs].
- Timing: set before first show; on already visible windows existing capture sessions may briefly keep content. [R]
- Transparent/layered caveats [I, test]: Tauri transparent windows on Windows use WebView2 with DirectComposition, per-pixel alpha; affinity applies to the HWND regardless. Known visual quirk is the DWM shadow/border on undecorated transparent windows (cosmetic, set `shadow: false`). I found no source claiming that transparency breaks exclusion; but `WS_EX_LAYERED` + affinity needed Win8+ (pre-8 returned error). Verify: set affinity, then run BitBlt/WGC capture and diff.
- Undocumented `WCA_EXCLUDED_FROM_DDA` (SetWindowCompositionAttribute, Win10 1709+) only affects Desktop Duplication; useless as a fallback beyond Win10 <2004. [R]
- Policy note: some corporate MDM/group-policy configs may constrain behavior [R: getdusky].

---

## 2. Doing it in Tauri 2

### 2.1 What Tauri/tao actually do [V: read tao `dev` branch source]

- macOS: builder `content_protected(true)` -> `ns_window.setSharingType(NSWindowSharingType::None)` at creation; runtime `set_content_protected(bool)` toggles `.None` / `.ReadOnly`.
- Windows: `set_content_protection(true)` -> `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`, **return value is discarded** (`let _ =`), so failure (e.g. old Windows build) is silent. Applied in window init after creation, before `set_visible`.
- `skip_taskbar` on Windows: `ITaskbarList::DeleteTab(hwnd)`; this is not `WS_EX_TOOLWINDOW`, does not guarantee Alt+Tab removal and can be undone by shell restarts/AddTab. On macOS tao has no `skip_taskbar` handling in the window file I grepped (activation policy is app-level, separate API).
- `focusable(false)` on Windows sets a window flag (`FOCUSABLE`); on macOS `set_focusable` exists but Electron docs note it does not remove existing focus. `set_ignore_cursor_events` exists on both.
- Raw handles: `WebviewWindow::hwnd()` (Windows), `ns_window()` (macOS) give native handles for anything Tauri doesn't expose. Use the `windows` crate and `objc2-app-kit`.

### 2.2 Useful crates / projects

- `tauri-nspanel` (ahkohd, v2.1, Tauri 2): converts/creates `NSPanel` windows (`PanelBuilder`, `tauri_panel!` macro with `can_become_key_window`, `is_floating_panel`, levels, `show` vs `show_and_make_key`). 427 stars, Apache/MIT, used by shipping apps. [V: README]. Pluely (Tauri Cluely clone) credits it. [V: README mention only; Pluely v1 is closed source so its stealth internals are unverifiable.]
- `window-vibrancy` (Tauri org): NSVisualEffectView on macOS, Acrylic/Mica/Mica-tabbed on Windows. [I/known]
- `tauri-plugin-global-shortcut` (Carbon `RegisterEventHotKey` on mac, `RegisterHotKey` on Win): no permissions, no focus steal.
- `screencapturekit` crate (docs.rs, 8.x) exposes SCK incl. screenshot manager. [V: exists on docs.rs; API details not read]
- `objc2` / `objc2-app-kit` for sharingType, collectionBehavior, level, styleMask.

### 2.3 Clone projects: what I could and couldn't learn

- **Pluely** (Tauri): README claims excluded from capture, no focus steal, icon can vanish from Dock/taskbar; credits tauri-nspanel. v1 closed source, so I can't tell what broke. [V]
- **glass (pickle-com)**: Electron. `github.com/pickle-com/glass` page fetch returned 404 at the URL I tried and raw file guess 404; I could not read its stealth code. Electron equivalents are `setContentProtection`, `setVisibleOnAllWorkspaces({visibleOnFullScreen:true})`, `setAlwaysOnTop('screen-saver')`, `skipTaskbar`; Electron docs warn `setVisibleOnAllWorkspaces` switches process type and "hides the window and dock for a short time every time it is called" - **call once**. [V: Electron docs]
- **cheating-daddy** (Electron): README only lists always-on-top, click-through mode, audio capture; no stealth internals documented. [V]
- **Cue** (Swift, abbinavv/cue): floating `NSPanel`, `setActivationPolicy(.accessory)`, `sharingType=.none`, manual "invisibility toggle", needs Screen Recording + Accessibility; honest README: not hidden from OBS/QuickTime on 15+. No process disguise. [V]
- **Electron fix attempt**: a commit "hide content protected windows during screen capture with ScreenCaptureKitMac" (for Electron's own `desktopCapturer` only; works only for windows protected before stream start) and a macOS content-protection refactor (#46813) that was reverted within days (May 2025). Lesson: even Chromium-side, SCK exclusion is order-sensitive. [V: titles only, mirrors; could not read bodies]

---

## 3. The critical requirement: overlay stays excluded while a normal main window is open

### 3.1 What the platform guarantees

- Both OS flags are **per-window** (HWND display affinity; NSWindow sharingType). Neither is per-process. So two windows with different flags is supported by design. [V for Windows docs "top-level window... belongs to calling process"; I for mac]
- Therefore the predecessor's breakage was almost certainly in *our* code path, not the OS. I couldn't see the predecessor's code, so these are ranked hypotheses [I]:
  1. **Window recreation.** Tauri destroys/recreates the native window (close vs hide, `WebviewWindowBuilder` rebuilt by label, hot reload, React effect re-creating) and the flag is only applied in one place (config at creation, or one JS call on startup). New HWND/NSWindow starts with defaults.
  2. **Config applied globally or to the wrong window.** `contentProtected` in `tauri.conf.json` is per window entry, but JS `getCurrentWindow().setContentProtected()` runs against whichever window the script is in; a shared frontend bundle toggling "stealth" for both windows, or a settings switch ("hide main from capture") overwriting the overlay.
  3. **Capabilities/permissions.** In Tauri 2, `core:window:allow-set-content-protected` must be granted to each window label via capability files; a second window without it fails (promise rejects, easily swallowed).
  4. **Silent failure.** tao discards the Win32 return value; JS errors swallowed; no read-back.
  5. **Toggle races.** One window used for both modes and flipped on `show/hide` or focus; on Windows setting on a visible window may leak frames to existing capture sessions.
  6. **macOS AppKit resets.** One research repo claims AppKit can reset sharingType during some window operations [R, unverified: privateai0 repo summary]. Plausible triggers: style-mask changes, converting NSWindow to NSPanel (class swap), collection-behavior/level changes, fullscreen/Space transitions, `setActivationPolicy` flips. Assume any such operation may require re-applying.
  7. **Child/popup windows.** Context menus, tooltips, dialogs, devtools, native menus spawned by the overlay are separate windows without the flag (Electron #45971 is this class for macOS menus).
  8. **WebView2/WKWebView:** extra native surfaces (e.g. `<select>` popups, IME candidate window, drag images) may be separate OS windows; keep the overlay UI to in-page HTML only.
  9. **macOS SCK inconsistency** (Apple forum 808016): even correctly flagged windows may be shown or hidden depending on capturer filter timing. Not our bug but it will look like one in testing.

### 3.2 Robust design

1. **Two permanent windows with fixed labels:** `overlay` (stealth) and `main` (normal). Never reuse one window for both roles. Different capability files: `capabilities/overlay.json` (grant set-content-protected, set-ignore-cursor-events, set-always-on-top etc.) and `capabilities/main.json` (no stealth permissions at all, so the main UI cannot toggle it).
2. **Own the flag in Rust, not the frontend.** A `StealthGuard` module owns `overlay` and exposes only `apply()`, `verify()`. The frontend never calls content-protection APIs.
3. **Create the overlay in Rust with the flag set at build time** (`.content_protected(true)`, `.visible(false)`), then immediately call the native apply again (`SetWindowDisplayAffinity` with a checked return; `setSharingType`) before first `show()`.
4. **Never destroy the overlay.** Hide via `hide()` / `orderOut`. Intercept `CloseRequested` -> `prevent_close()` + hide. Handle `WindowEvent::Destroyed` defensively: if it fires, recreate through the same constructor.
5. **Re-apply and verify on events** (cheap, idempotent): window `Focused`, `Moved`, `Resized`, `ScaleFactorChanged`, `ThemeChanged`, after any show, after setActivationPolicy/level/collection-behavior/style-mask changes or NSPanel conversion, display configuration change (`NSApplicationDidChangeScreenParameters`, `WM_DISPLAYCHANGE`), Space change (`NSWorkspaceActiveSpaceDidChangeNotification`), wake from sleep/session unlock (`WM_POWERBROADCAST`, `NSWorkspaceDidWakeNotification`), and a **1 s watchdog timer** while the overlay is visible.
6. **Read-back verification:** Windows `GetWindowDisplayAffinity` must equal `0x11`; macOS `sharingType == .none`. On mismatch: re-apply, and if still mismatched **fail closed** (hide overlay + toast "stealth unavailable on this OS version").
7. **Fail-closed order of operations:** apply flag -> verify -> only then show. Never show first.
8. **Popups:** disable native context menus on overlay (`contextmenu` preventDefault); no native menus/dialogs from overlay; any helper window created by overlay must go through the same `StealthGuard::protect(window)`.
9. **Main window never flagged by default** (user-optional "hide main window too" uses the same guard keyed per label).
10. **Log + self-test:** the StealthGuard logs state transitions; a debug "stealth self-test" mode (section 8) paints a sentinel pattern and captures the screen through several APIs to prove exclusion on this machine, shown as green/red badge in settings.

---

## 4. Hiding from Dock / taskbar / Alt+Tab / Cmd+Tab / tray; non-focus-stealing; click-through; above fullscreen

### 4.1 macOS

- App-level, not window-level: `NSApp.setActivationPolicy(.accessory)` removes Dock icon, Cmd+Tab entry and the menu bar for the app. Tauri: `app.set_activation_policy(ActivationPolicy::Accessory)`. Alternative static: `LSUIElement=true` in Info.plist.
- Consequence for two-window design [I]: the Dock icon is **per app**. While only the overlay is on screen use `.accessory`; when the user opens the main window switch to `.regular` (Dock icon appears, which is fine since main window is not stealth). Flipping the policy can briefly hide windows/Dock, and "Accessory + WKWebView element fullscreen leaves Dock hidden" is a reported bug on macOS 27 (FB24577577). [R: Apple forum thread 846037] Re-apply sharingType, level, collection behavior after every flip (3.2 item 5). Alternatively keep Accessory permanently and open main from a global hotkey; simplest and avoids flips (recommended default for "call mode").
- No menu-bar tray item in stealth: the menu bar is part of display capture, so a status item would show in a shared screen. Optional tray only in non-stealth mode.
- Non-focus-stealing overlay: use an **`NSPanel` with `.nonactivatingPanel` style mask** (tauri-nspanel), `canBecomeKey` false by default (true only while a text field needs input, via `becomesKeyOnlyIfNeeded`), `hidesOnDeactivate=false`. An ordinary NSWindow activates the app when ordered front, which can pull Zoom out of fullscreen Space. [R: Apple forum: overlay above all windows even when moving spaces]
- Above fullscreen apps/Spaces: Accessory policy + panel + `collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]` + window level `.screenSaver` or `.statusBar`+ (Electron: `visibleOnFullScreen:true`; levels `floating`..`status` sit below Dock/menu, `screen-saver` above; Apple discourages beyond screen-saver+1). [V: Electron docs; I for panel specifics]. pierce.dev's PoC used `assistiveTechHighWindow` level + `.ignoresCycle`; it found a very high level makes the window vanish rather than show black on legacy paths, with no documented guarantee. [R]
- Zoom's own fullscreen/share toolbars live at high levels; test that overlay level is above Zoom's floating toolbar if desired.
- Hotkeys without focus: Carbon hotkeys (tauri-plugin-global-shortcut) need no permission. Mouse side buttons / arbitrary mouse binds need a `CGEventTap` (Input Monitoring permission) [I].
- Click-through: `ignoresMouseEvents = true`. For "clickable only when hovering controls" use a global `NSEvent.addGlobalMonitorForEvents(.mouseMoved)` (mouse-moved global monitors work without Accessibility permission [I, verify]) to toggle `ignoresMouseEvents` when the cursor enters interactive rects; plus a hotkey toggle. Note: while ignoring events, JS hover never fires.

### 4.2 Windows

- Taskbar + Alt+Tab + Win+Tab: set `WS_EX_TOOLWINDOW` and clear `WS_EX_APPWINDOW` via `SetWindowLongPtrW(GWL_EXSTYLE)` + `SetWindowPos(SWP_FRAMECHANGED|SWP_NOMOVE|...)`. Do this **before first show**, or hide/show once. Tauri's `skip_taskbar` (ITaskbarList::DeleteTab) is a taskbar-only fix; use it as belt, TOOLWINDOW as braces. [V code, I behavior]
- No focus steal: `WS_EX_NOACTIVATE` (Tauri `focusable(false)` sets the flag [V]) and show with `SW_SHOWNOACTIVATE`; clicks on a NOACTIVATE window still deliver mouse input without taking foreground. Text input into the overlay then needs a temporary activation toggle; prefer hotkey-driven UI.
- Click-through: `WS_EX_TRANSPARENT | WS_EX_LAYERED` (`set_ignore_cursor_events`); same hover-rect toggle pattern with a low-level mouse hook (`WH_MOUSE_LL`), which also gives XBUTTON1/2 side-button binds without focus.
- Always on top above fullscreen: `HWND_TOPMOST`; works over borderless/windowed-fullscreen apps (Zoom share, PowerPoint slideshow generally); exclusive-fullscreen DirectX apps can occlude it. Re-assert topmost on a timer/foreground-change hook (`SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`) because other topmost windows (Zoom toolbar) reorder.
- Tray: don't create one in stealth mode. Process still visible in Task Manager (explicitly accepted by BRIEF).
- WebView2 child processes show as `msedgewebview2.exe` with user-data-dir under `%LOCALAPPDATA%\<identifier>\EBWebView` -> the app identifier leaks there (section 5).

---

## 5. Process disguise (user-renamable name/icon, "copy another app's look")

Principle: cosmetic and user-initiated, local only, nothing that defeats security tooling. Honest limits: Activity Monitor/Task Manager/`ps` still show a process; command line and file path still reveal real binary lineage; EDR on managed machines will see the original file hash/signature and copied-binary behavior.

### 5.1 macOS

What users see: Dock/Cmd+Tab name+icon (bundle), menu bar app menu title (`CFBundleName`), Activity Monitor name (executable/bundle name), Force Quit list, Login Items name.

- Runtime-only (no copy): `NSApp.applicationIconImage = ...` changes Dock/Cmd+Tab icon while running; window titles; in-app menu title via menu construction. Does **not** change Activity Monitor or `ps` name. [I/known]
- Full disguise = **build a shadow bundle**: copy `Lowk.app` to `~/Library/Application Support/Lowk/Disguise/<Name>.app`, rewrite `Info.plist` (`CFBundleName`, `CFBundleDisplayName`, `CFBundleExecutable` + rename `Contents/MacOS/<exe>`, `CFBundleIdentifier`, `CFBundleIconFile` + replace `.icns`), re-sign ad-hoc (`codesign --force --deep -s -`), clear quarantine if present, launch via `open`, quit original. Ad-hoc re-sign is required on Apple Silicon (modified bundle invalidates signature; arm64 code must be signed to run). Locally generated, non-quarantined bundle is not subject to Gatekeeper assessment on first launch [I, verify on 14/15/26/27].
- **TCC permissions are bound to bundle id + code identity**: each disguise copy (new ad-hoc cdhash/bundle id) loses Screen Recording / Microphone / Accessibility / Input Monitoring grants and shows a new permission prompt, and Sequoia+ re-asks screen-recording monthly. [R: tidbits; I for binding] So: choose disguise during onboarding **before** requesting permissions; changing it later = re-grant flow; document it. Consider keeping the bundle id stable and only changing display strings/executable name (TCC with ad-hoc signing still keys on cdhash, so changes anyway) - verify. A stable self-signed/Developer ID identity later improves this (BRIEF defers signing).
- "Copy another installed app": read `/Applications/X.app/Contents/Info.plist` (`CFBundleName`, `CFBundleDisplayName`, `CFBundleIconFile`/`CFBundleIconName`) and copy its `.icns` (asset-catalog `Assets.car` icons need `NSWorkspace.icon(forFile:)` rendering to build a new icns instead). Feasible. Risks: trademark/impersonation, EDR "masquerading" heuristics (MITRE ATT&CK T1036.005), user confusion with two apps of the same name (Spotlight, Launch Services duplicates; `CFBundleIdentifier` must differ). Guardrails: never offer system/security apps (Finder, System Settings, Activity Monitor, antivirus/EDR), local only, explicit confirm, show what will be copied.
- Third-party helper names: WKWebView helpers appear as `com.apple.WebKit.*`; fine.
- Gatekeeper/AV risk: low for local non-quarantined ad-hoc bundles; moderate if distributed pre-disguised (don't).

### 5.2 Windows

What users see: Task Manager "Name" column = `FileDescription` (version resource), Details tab = exe file name, icon = exe icon resource, taskbar/Alt+Tab icon = window icon (runtime) / AppUserModelID, tray tooltip (runtime).

- Runtime (no copy): window titles, `WM_SETICON`, taskbar overlay icon, `SetCurrentProcessExplicitAppUserModelID`. Cheap and AV-neutral.
- A running exe can't rename itself (file locked). Approach: **copy-and-relaunch launcher**: copy `Lowk.exe` to `%LOCALAPPDATA%\<Folder>\<Name>.exe` (plus resources dir), optionally patch resources on the *copy* with `BeginUpdateResource/UpdateResource` (icon group, `VS_VERSIONINFO` FileDescription/ProductName/CompanyName), start it, exit original. Patching invalidates any Authenticode signature (we're unsigned now).
- Cheaper variant: ship with a neutral built-in `FileDescription` and a user-changeable exe *file name*; patch only icon on copy. Fewer PE edits = fewer heuristics.
- AV/SmartScreen: SmartScreen reputation applies to files with Mark-of-the-Web; a locally copied file has none. Defender ML/heuristics sometimes flag "self-copy to AppData + resource patch + relaunch" as dropper-like and unsigned, newly-seen hashes get low reputation. Expect false positives for some users; provide a no-patch mode and document exclusions. Copying another app's icon/version info (e.g. from `C:\Program Files\...\app.exe` via `ExtractIconEx` and reading its version resource) is feasible but is the same masquerading concern as on mac; apply the same guardrails.
- WebView2: data folder `%LOCALAPPDATA%\<identifier>` and `msedgewebview2.exe` command lines carry the app identifier; set a neutral identifier / explicit `WEBVIEW2_USER_DATA_FOLDER` per disguise (changing it drops cookies/localStorage unless migrated).
- Registry/Start Menu/Run key names: avoid creating autostart entries with the disguised name that collide with real apps.

### 5.3 Build-time vs runtime summary

| Item | macOS runtime | macOS needs copy | Windows runtime | Windows needs copy |
|---|---|---|---|---|
| Dock/taskbar icon | yes | for Finder/Force Quit/Login Items | yes (window icon) | for Task Manager icon |
| Menu/window title | yes | `CFBundleName` for app menu | yes | - |
| Process/exe name | no | yes | no | yes |
| Task Manager/Activity Monitor display name | no | yes | no | yes (version resource) |

---

## 6. Adaptive coloring

### 6.1 Capturing what is behind the overlay

- If exclusion works, the screen already doesn't contain our window, so sampling the rect under it is "free". **Don't depend on that on macOS**: SCK exclusion by sharingType is inconsistent. Instead explicitly exclude our window(s) in the filter (`SCContentFilter(display:excludingWindows:[overlaySCWindow])`) which is a documented, deterministic mechanism. [I; the thread-808016 sample uses `excludingApplications`, which also works]
- **macOS:** `SCScreenshotManager.captureImage(contentFilter:configuration:)` (macOS 14+) with `configuration.sourceRect = overlay frame (display-local, points)` and `width/height` set tiny (e.g. 32x20) so the downscale happens on GPU; `showsCursor=false`; `capturesAudio` n/a. One-shot per tick; avoid a persistent `SCStream` (keeps capture session and system recording indicator alive, higher power). Avoid deprecated `CGWindowListCreateImage` (triggers the Sequoia "bypass system window picker" alert per Apple DTS via TidBITS) [R]. Permission: **Screen & System Audio Recording** (TCC); preflight with `CGPreflightScreenCaptureAccess()`, request with `CGRequestScreenCaptureAccess()` (DTS-recommended over probing `CGWindowListCopyWindowInfo`, whose gating changes between Tahoe and the 27 beta). [V: Apple forum thread 839069] Sequoia+ shows periodic re-confirmation prompts and a menu-bar recording indicator while capturing; one-shot screenshots may still trigger it briefly [unverified, test]. Fallback with zero permission: see 6.4.
- **Windows:** in order of preference
  1. `BitBlt/StretchBlt(HALFTONE)` from the screen DC (`GetDC(NULL)`) of the overlay rect into a 32x20 DIB; no permission, no border, no setup; CPU a few ms for a small rect [I estimate]. Without `CAPTUREBLT`, layered windows aren't included; affinity excludes ours anyway. Pure-black result can mean protected/overlay video -> treat as "unknown", keep previous value.
  2. DXGI Desktop Duplication (no yellow border, no permission, GPU texture crop). Beware the 24H2 quirk where frames wake with no content. [R]
  3. Windows.Graphics.Capture (yellow capture border unless `IsBorderRequired=false`, which needs the borderless-capture capability on recent Win11); keep as last resort.
- Multi-monitor / DPI: sample in physical pixels of the monitor the overlay mostly intersects; re-sample on move/resize/display change.

### 6.2 Cost [I, estimates - measure]

- 0.5 Hz one-shot, ~32x20 output: well under 1% CPU on Apple Silicon/modern x86; the dominant cost is the capture session setup (SCK ~5-20 ms per screenshot). Skip sampling when: overlay hidden, screen locked, display asleep, on battery saver (drop to 5 s), or when the rect and the last frame hash are unchanged (cheap early-out).
- Don't run the analysis in the WebView; do it in Rust, send one small JSON `{bg_luma, text_mode, tint}` to the UI via event.

### 6.3 Algorithm

1. Downscale rect to ~24x16; convert sRGB -> linear; relative luminance `Y = 0.2126R + 0.7152G + 0.0722B` (WCAG 2.x).
2. Robust stats over pixels: median Y, p10, p90, plus mean color (OKLab) and a "busyness" = p90-p10. Median of last 3 samples to reject flicker (video, scrolling).
3. **Text color decision** with hysteresis: WCAG contrast `(L1+0.05)/(L2+0.05)`. White-vs-black crossover is Y≈0.179. Use a Schmitt trigger: switch to dark text only above Y≈0.22, to light text only below Y≈0.14.
4. **Scrim (background tint)**: compute the minimal alpha `a` of a scrim color `S` (near-black for light text, near-white for dark text, optionally tinted with 10-15% of the sampled hue/chroma so colored content looks coherent) such that the contrast between the chosen text color and the *worst-case* blended background `a*S + (1-a)*bg_worst` is >= 4.5:1 (WCAG AA body text), clamped to alpha range [0.35, 0.85] to keep it "semi-transparent". Worst-case bg = p90 Y when text is light, p10 when dark, so one bright pixel cluster behind text doesn't kill legibility. High busyness raises `a` and adds backdrop blur.
5. **Border/outline**: 1 px border = text color at ~30% alpha plus an inner 1 px hairline of the scrim color (dual ring) so it's visible on any background; add faint text-shadow/halo for large display text only.
6. **Smoothing**: ease to targets in OKLab with an exponential filter (tau ~0.5 s) and CSS `transition: 600-800ms`; never animate faster than the 2 s sampling period; ignore changes < ~0.03 Y (deadband).
7. Sample again immediately (debounced 150 ms) when the window moves/resizes, in addition to the 2 s timer.
8. User controls: auto / fixed-light / fixed-dark, min/max opacity, "reduce transparency".

### 6.4 Zero-permission baseline

- macOS `NSVisualEffectView` (`.hudWindow` / `.underWindowBackground`, `blendingMode=.behindWindow`) and Windows Acrylic/Mica (`window-vibrancy`) blur+tint the backdrop in the compositor with no screen-recording permission and no CPU. They adapt to dark/light automatically but don't tell the app the luminance, so text color can't be chosen exactly. Recommended: ship vibrancy as the baseline and the sampler as an enhancement when permission is granted. Test that vibrancy doesn't interact badly with content protection (not found in sources) and Win10 Acrylic is laggy on drag (known) [I].

---

## 7. Recommended implementation plan (per OS)

### Shared
1. Two native windows, fixed labels `overlay` and `main`; separate capability files; overlay created in Rust, never destroyed (3.2).
2. `StealthGuard` (Rust): `apply/verify/failclosed`, event + 1 s watchdog re-apply, structured log, UI state "Stealth: verified / degraded / unavailable".
3. Panic hotkey (hides overlay instantly, configurable), plus "hide overlay when main app loses/gains X" optional.
4. Debug self-test harness (section 8) in the repo, run in CI on Windows runners where possible and manually per release on macOS.
5. Disguise manager as a separate module with explicit guardrails (no system/security app names).

### macOS 14+ (Apple Silicon)
1. Tauri `content_protected(true)` on `overlay` + native re-apply (`sharingType=.none`) before show; verify read-back.
2. Convert overlay to `NSPanel` (tauri-nspanel): `.nonactivatingPanel`, level `.screenSaver`, collection behavior `[.canJoinAllSpaces,.fullScreenAuxiliary,.stationary,.ignoresCycle]`, `hidesOnDeactivate=false`. Re-apply sharingType after conversion.
3. Activation policy: default Accessory in call mode, Regular only while main window is open (or main opened by hotkey in Accessory). No tray in stealth. Re-apply after each flip.
4. Hotkeys via Carbon; side mouse buttons via CGEventTap (Input Monitoring).
5. Click-through with hover-rect toggling.
6. Capture-exposure mitigation: panic hotkey; optional "auto-hide while a share is detected" heuristic (enumerate windows of Zoom/Teams/Meet-in-Chrome share toolbars/indicators by owner+title; brittle, ship as opt-in, localized strings). No public API reveals "someone is capturing".
7. Adaptive color: `SCScreenshotManager` sampler with explicit `excludingWindows`; vibrancy baseline; handle missing permission gracefully.
8. Disguise: onboarding-time choice, shadow bundle with ad-hoc re-sign; warn about TCC re-grant.
9. Docs/UI: state plainly that on macOS 15+ the overlay may be visible to ScreenCaptureKit-based recorders (OBS/QuickTime, maybe Zoom/Teams) and only some browser-based shares hide it; link to the test matrix.

### Windows 10 2004+ / 11
1. `content_protected(true)` on overlay + own `SetWindowDisplayAffinity(0x11)` with **checked** return and `GetWindowDisplayAffinity` read-back; if build < 19041, refuse to show overlay in stealth (WDA_MONITOR would show a black box).
2. Before first show: `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, drop `WS_EX_APPWINDOW`; also `skip_taskbar`.
3. `HWND_TOPMOST` with foreground-hook re-assertion; `SW_SHOWNOACTIVATE`.
4. Click-through via `WS_EX_TRANSPARENT|WS_EX_LAYERED` and WH_MOUSE_LL for hover/XButton binds.
5. Sampler: BitBlt small rect, DDA fallback; acrylic/mica optional baseline.
6. Disguise: runtime icon/title/AUMID default; optional copy-and-relaunch with resource patch behind an "advanced" switch; neutral WebView2 identifier.
7. Test on Win10 22H2 and Win11 23H2/24H2/25H2, with and without hardware acceleration in Discord/Chrome.

---

## 8. Verification test (build before features)

Goal: prove, on each target machine, that the overlay is absent from each capturer while the unprotected main window is present.

- **Setup:** two windows. `overlay` paints a high-entropy sentinel (e.g. saturated magenta/cyan checker) in stealth mode; `main` paints a different sentinel and is not protected. Place them non-overlapping.
- **Automated capture probes** (run inside the app or a helper binary): Windows - BitBlt, PrintWindow, DDA, WGC; macOS - `SCScreenshotManager` (display filter with/without exclusions, one-shot), `SCStream` (start/stop, run twice - the 808016 bug shows Stop/Start changes outcome), `CGWindowListCreateImage` (legacy), `screencapture -x`. Pass = main sentinel found, overlay sentinel absent.
- **Manual/semiautomated matrix** per release (screen record the shared output, or use a second device/second Zoom account viewing): Zoom (full-screen share, window share, "advanced capture with/without window filtering"), Google Meet in Chrome (stable + current Chrome flags), Teams (new), Discord (Go Live), OBS (macOS Screen Capture source, Display Capture; Windows Display/Window/Game Capture; WGC and DXGI methods), QuickTime, Slack huddle, native screenshot. macOS: 14, 15, 26, and 27 (current as of Oct 2026) on Apple Silicon.
- **Lifecycle stress:** open/close main window 50x; hide/show overlay; toggle activation policy; switch Spaces; enter/leave fullscreen Zoom; sleep/wake; unplug/plug monitor; DPI change; app relaunch. After each step re-run the probes. Any failure -> record which event preceded it and add re-apply on that event.
- **Runtime:** keep the in-app badge (verified/degraded) driven by read-back, plus an on-demand "run capture probe" button.

---

## 9. Open questions / risks

1. macOS 15/26/27 SCK behavior is the single largest uncertainty; sources conflict and I could not test. Re-run the matrix on macOS 27 now. Apple may change it again at any release.
2. Chrome moving `getDisplayMedia` to SCK by default would expose the overlay in Meet.
3. TCC identity churn from disguise copies on macOS will annoy users; may need signing sooner than BRIEF assumes.
4. Windows AV false positives on copy-patch-relaunch disguise.
5. Corporate machines: EDR/MDM sees all of it; capture-exclusion might be restricted by policy.
6. Legal/ethical: recording consent warning already planned; also note that using the overlay where rules prohibit assistance (exams, interviews) is the user's responsibility; keep marketing claims honest ("hidden from many screen-share paths", not "undetectable").

---

## Sources

- Microsoft Learn, SetWindowDisplayAffinity: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity
- Habr, capture exclusion tests (RU): https://habr.com/post/1025310/
- Apple Dev Forums 808016 (SCK + NSWindowSharingNone, FB21115847): https://developer.apple.com/forums/thread/808016
- Apple Dev Forums 839069 (CGWindowListCopyWindowInfo permission gating, Tahoe vs 27 beta): https://developer.apple.com/forums/thread/839069
- Apple Dev Forums 846037 (Accessory + WKWebView fullscreen Dock bug, macOS 27): https://developer.apple.com/forums/thread/846037
- Apple Dev Forums 826308 (overlay above all windows/Spaces): https://origin-devforums.apple.com/forums/thread/826308
- Tauri issue #14200: https://github.com/tauri-apps/tauri/issues/14200
- Electron issue #19880: https://github.com/electron/electron/issues/19880 ; #45971 https://github.com/electron/electron/issues/45971
- Electron BrowserWindow API docs: https://www.electronjs.org/docs/latest/api/browser-window
- Electron commits (mirror): https://ayakael.net/mirrors/electron/commit/fd88908457a986f06e310e80499e4b2775d8ba70 ; PR #46813 https://github.com/electron/electron/pull/46813
- tao source (macOS/Windows window.rs, dev branch): https://github.com/tauri-apps/tao
- tauri-nspanel: https://github.com/ahkohd/tauri-nspanel
- Pluely: https://github.com/iamsrikanthnani/pluely
- cheating-daddy: https://github.com/sohzm/cheating-daddy
- Cue: https://github.com/abbinavv/cue
- pierce.dev, invisible mac app PoC: https://pierce.dev/notes/building-a-kind-of-invisible-mac-app
- Zoom community, flicker with NSWindowSharingNone: https://community.zoom.com/t5/Zoom-Meetings/Flickering-window-in-the-screen-capture-of-Zoom-Mac-desktop/m-p/110428
- TidBITS, Sequoia permission prompts / DTS on legacy capture APIs: https://talk.tidbits.com/t/apple-reduces-excessive-sequoia-permission-requests-shifts-to-monthly/28621
- Dusky write-up: https://getdusky.app/blog/is-cluely-detectable/ ; Abedini write-up: https://raoufabedini.dev/write-ups/invisible-window-research
- WebRTC SCK capturer commit: https://webrtc.googlesource.com/src/+/d4a6c3f76fc3b187115d1cd65f4d1fffd7bebb7c
- screencapturekit Rust crate: https://docs.rs/screencapturekit
- Not fetched (flagged): pickle-com/glass source (404 on guessed URLs); privateai0/macos-window-privacy-research (declined to summarize - proctoring-evasion purpose); MITRE T1036.005 cited from memory.
