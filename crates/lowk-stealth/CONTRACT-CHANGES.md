# lowk-stealth: contract-change proposals (for the architect / lowk-types owner)

Nothing outside `crates/lowk-stealth/**` and `tools/stealth-probe/**` was edited. All items work today
with the current `lowk-types`; the proposals remove awkwardness.

1. **`OverlayController::move_hold(phase, &dyn CursorSource)`**: a borrowed cursor cannot outlive the call,
   but the follow loop runs for as long as the bind is held. Implemented as: the borrowed source gives the
   initial grab sample (no jump); the loop samples a native cursor (`CGEventGetLocation` / `GetCursorPos`).
   Proposal: take `Arc<dyn CursorSource>`. The concrete `OverlayCtl::move_hold_with(phase, Arc<dyn CursorSource>)`
   already exists (used by tests and the fixture).
2. **`Tone` gets `scrim_min: f32`** (0..1): minimum scrim opacity that keeps WCAG 4.5:1 for the worst-case
   backdrop pixel (`AdaptiveTone::min_scrim_alpha()`). The UI should use `max(k * .85, scrim_min)` for the scrim.
   Not emitted today (field absent), so contrast is only as good as the user opacity slider allows.
3. **`OverlayController` additions** (currently inherent methods on `OverlayCtl`, reachable via `Stealth.overlay`):
   `escape()` (Esc in the interactive overlay), `on_window_event(WindowEventKind)` (blur exits interactive;
   throttled re-apply). `Stealth` also has `set_event_sink(Arc<dyn EventSink>)` (emits
   `AppEvent::Overlay(Mode)` and deduplicated `AppEvent::Stealth`), `set_hide_main(bool)`, `set_tone_mode`,
   `spawn_tone_sampler(Arc<dyn RegionSampler>)`. `create_stealth` keeps the spec signature.
4. **Self-test pixel layer**: `OverlayController::selftest()` always checks the read-back flags and (macOS)
   `kCGWindowSharingState`. The pixel layer needs a grab *without* window exclusion, which `RegionSampler`
   (excludes our windows) cannot do, and a sentinel painted by the UI. Proposal: add
   `trait ScreenGrab { fn grab(&self, rect: Rect) -> Result<(Vec<u8>, u32, u32)> }` to `lowk-types`
   (implemented by `lowk-capture`, no exclusion); until then `lowk_stealth::SelfTestCapture` is the same trait
   and is injected with `OverlayCtl::set_selftest_capture`. The overlay UI must paint a magenta sentinel
   (`r>170, b>170, g<100`) while the test runs.
5. **`RawWindows`/`RawWindow`** are defined in `lowk-stealth` (wrapping `raw_window_handle::RawWindowHandle`);
   `lowk-app` builds them from `window.window_handle()` (AppKit `ns_view` or Win32 `hwnd`). `main` and `pill`
   are optional. Each label is registered separately; a failure of `main` never touches `overlay`.
6. **macOS panel**: `tauri-nspanel` is not used. The overlay's class is swapped to `LowkPanel` (an `NSPanel`
   subclass overriding `canBecomeKeyWindow`, answered from a per-window allow-list) exactly as in the spike.
   Risk (KVO-wrapped `NSKVONotifying_*` classes) is the same as noted in the architecture; `deny.toml`
   needs no git allowance.
7. **Windows**: `WM_DISPLAYCHANGE` cannot be observed without owning the window proc; the app should call
   `Stealth::on_window_event(label, DisplayChanged)` from its event loop. `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`
   is installed inside `create_stealth` and must be created on a thread that pumps messages (the UI thread).
8. `justfile` recipe suggestion (root file, not edited): `stealth-probe: tools/stealth-probe/run.sh`.
