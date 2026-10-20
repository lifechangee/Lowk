# M0 decisions (WP0 / WP1)

Ambiguities in `ARCHITECTURE.md` resolved by the simplest option. Architect: veto by PR.

## Contract (`lowk-types`)
1. **Overlay visibility**: `OverlayVisibility = always | recording_only | manual | pill`, default `recording_only` (owner decision). `OverlaySettings.pill_edge` (default `right`) added for `pill`.
2. **`MainThread`** is object-safe: trait has `run(Box<dyn FnOnce>)` + `is_main()`; `run_sync<R>` is an inherent method on `dyn MainThread` (the spec's generic trait method is not object-safe but is used as `Arc<dyn MainThread>`).
3. **`SettingsPatch`** is an RFC 7386 JSON merge patch (`serde_json::Value`, TS type `unknown`). `Settings::apply_patch` validates by re-deserializing.
4. **Bind keys** use W3C `KeyboardEvent.code` strings (`KeyS`, `Enter`, `Backslash`); `Mods` is four booleans (`meta` = Cmd/Win). `Settings::defaults_for(Os)` gives per-OS default binds (`Settings::default()` uses the current OS).
5. **Tuple variants** of the spec (`Perm(PermKind, PermState)`, `Level{..}`) became struct/newtype variants with adjacent tagging (`{type, data}`) so specta/TS output is stable.
6. **`Frame16k`** lives in `lowk-types` (owned `Vec<f32>`, not a pooled buffer) so `lowk-stt` need not depend on `lowk-audio`. Pooling can replace it later without changing field names.
7. **Vault path default** is the symbolic `~/Documents/Lowk`; `lowk-platform` resolves it (Windows known folder). System folder ids/dir names are constants (`SYS_UNSORTED_ID`, `UNSORTED_DIR`, `TRASH_DIR`).
8. `Fake*`/`Noop*` for contract traits (`InlineMainThread`, `NoopOverlayController`, `MemSettingsStore`, `NoopRetriever`, `FixedCursor`, `Collect*Sink`) live in `lowk_types::fakes` so any crate can use them. Also added traits `EventSink`, `RegionSampler`, `InputBackend` (spec lists them elsewhere).
9. Rust `u64` ids/timestamps (`MonoNs`, `UttId`) are exported to TS as `number` (`dangerously_cast_bigints_to_number`); safe below 2^53 (about 104 days of monotonic ns).
10. Settings structs are `#[serde(default)]`, so generated TS types mark their fields optional.

## Repo / tooling
11. **Toolchain** pinned to 1.99.1 (installed here); MSRV field `1.85`.
12. **Versions**: `tauri 2.12`, `tauri-specta =2.0.0-rc.25`, `specta =2.0.0-rc.25`, `specta-typescript =0.0.12` (exact pins, pre-release), `ort =2.0.0-rc.15`. `tauri-nspanel` (git) is left for WP2; `deny.toml` has `allow-git = []` until then. `tauri 3.0.0-alpha` exists on crates.io; we stay on 2.x.
13. **Bindings**: `cargo xtask bindings` runs `cargo run -p lowk-app --example export_bindings`, so xtask does not link Tauri. Commands are typed; events are exposed as the `AppEvent` DTO plus `EVENTS` name constants and a generic `on<T>()` helper (not per-event tauri-specta `Event` structs). `lowk_types::register_types` lists extra DTOs for export.
14. **Capabilities**: `build.rs` `COMMANDS` and `src/commands/mod.rs` must be kept in sync (append-only). Both capability files grant `core:event:default` (needed to `listen`) and no `core:window:*`.
15. **Windows config**: `contentProtected: true` and `visible: false` for the overlay are plain Tauri config; the real StealthGuard/verification is WP2. The overlay is declared in `tauri.conf.json` for M0 (WP6 moves creation into Rust).
16. **CSP** also allows `ws://localhost:1420` / `http://localhost:1420` for the Vite dev server (`connect-src`).
17. **Tauri hook cwd**: `beforeDevCommand` uses `pnpm --dir ../ui` (Tauri runs hooks from `crates/`, not from the config dir).
18. **deny.toml** ignores two unmaintained transitive advisories (RUSTSEC-2024-0436 `paste`, RUSTSEC-2024-0370 `proc-macro-error`) coming from tauri/specta. `multiple-versions = warn`.
19. **CI**: `lowk-app` is excluded from Linux jobs (needs WebKitGTK); it is linted/tested on macOS and Windows jobs (path-filtered on PRs, always on `main` and nightly). The bindings-staleness check runs in the macOS job. Playwright and `cargo about` jobs are left as TODO for WP10 / M4. `release.yml` and `nightly-probe.yml` are not created yet.
20. **Windows cross-check from macOS** needs a dummy `llvm-rc` on PATH (`printf '#!/bin/sh\nexit 0\n' > /tmp/fakebin/llvm-rc`), then `cargo check --workspace --target x86_64-pc-windows-msvc`.
21. **Frontend**: TypeScript pinned to `~6` (svelte-check does not support TS 7 yet); `pnpm@10.34.7`. i18n is split per namespace under `ui/src/lib/i18n/{ru,en}/<ns>.ts` (spec section 14); `ru` defines the key type and `en` must match (type + unit test).
22. `ui/` is its own pnpm root (`ui/pnpm-lock.yaml`), no root workspace file.

## Integration (WP6): contract changes applied to `lowk-types`
Proposals from the per-crate `CONTRACT-CHANGES.md` files (now deleted) were folded in:
23. **Hardware**: `HwReport`, `SttTier`, `RecReason`, `Recommendation` moved from `lowk-platform` into `lowk-types` (specta); `hw_probe` returns `HwReport`.
24. **Events**: `AppEvent::MyAnswer(QaUpdate)` (`qa://my-answer`) and `AppEvent::SttError(SttError)` (`stt://error`). `QaUpdate`, `QaRecord`, `CloseReason` moved to `lowk_types::qa`. `Session` publishes both on the bus (`subscribe_qa` stays as a convenience for tests).
25. **Call id**: `CallStore::begin_call -> (CallId, recorder)` is satisfied by `CallWriter::id()`; the app adapter is a thin wrapper, no store change needed.
26. **`NewCall.clock: Option<SessionClock>`**: `Vault::begin_call` uses it (else starts its own clock). `Session` always sets it.
27. **Bind phases** stay `Down`/`Up` (a hold is the time between them, auto-repeat suppressed); no `Held`. `InputStatus` stays Rust-only: `binds_get` returns `BindsState { binds, report }` instead. Key-code validation stays in `lowk-input` (`is_known_code`), `binds_set` rejects unknown codes before saving.
28. **`OverlayController::move_hold(phase, Arc<dyn CursorSource>)`**: the follow loop outlives the call. The app passes `lowk_input::SystemCursor`; the stealth crate no longer carries native cursor sources.
29. **`Tone.scrim_min`** (0..1) is emitted by the tone algorithm (re-emits when it moves by >= 0.05); the overlay scrim is `max(k * .85, scrim_min)`.
30. **`ScreenGrab`** trait (no window exclusion) lives in `lowk-types`; `lowk-stealth` uses it for the self-test pixel layer (`SelfTestCapture` removed). `RegionSampler` (excludes own windows) is the tone sampler's trait.
31. **Templates**: `default_templates()` includes `summary` («Резюме»); `lowk-store` no longer appends it.
32. **Not applied**: `StartOpts.demo_speed` (demo pace is `SessionDeps::demo_pace`), `Vault` extras stay in `lowk-store` (app uses them directly), `OverlayController` extras (`escape`, `on_window_event`, event sink) are reached through `Stealth.overlay` (`OverlayCtl`), since they are app-shell concerns.
33. `NewCall.started` stays UTC RFC 3339; local-offset needs a safe source from `lowk-platform` (post-M0).

## Integration (WP6): app shell
34. **Windows**: `tauri.conf.json` declares none; `lowk-app` creates `overlay` and `pill` hidden at startup (content-protected from birth), registers them with `StealthGuard` (apply -> verify) and only then may `show`. `main` is created on demand, registered/unregistered with the guard around its lifetime (`Stealth::register_main` / `unregister_main`, also on `Destroyed`). Dock icon (activation policy `Regular`) only while `main` exists, `Accessory` otherwise.
35. **State**: commands take `State<Arc<AppState>>`; stealth is a `OnceLock` so webviews that load before it exists do not panic. Every command is `#[tauri::command(async)]` (sync commands run on the main thread and would deadlock with `MainThread::run_sync`). Event-producing threads never call the main thread: visibility changes go through one `Driver` worker.
36. **Events** go out with `EventTarget::webview_window(label)`; a plain label also matches the window object and doubled every event. `overlay://*` reaches only the overlay; high-rate partial STT is coalesced to 20 Hz per utterance. `settings://changed` is new (both windows live-update language, opacity, templates).
37. **M0 sessions are demo sessions**: `lowk-audio` is still a no-op backend, so `session_start`, the record bind and the pill all play the scripted demo, and demo calls are written to the vault (`record_demo = true`) so library/CRUD have real content. M1 removes `effective()` in `commands/session.rs`.
38. **Combos** go through `tauri-plugin-global-shortcut` (`ComboHub` maps shortcut id -> action); hold, modifier-only and mouse binds through `lowk-input`'s hooks. Without Input Monitoring `MoveHold` reports `Degraded(InputMonitoring)` and nothing else is affected.
39. **Tone sampler** runs in `lowk-stealth` every ~2 s; macOS source is a ScreenCaptureKit one-shot excluding all windows of our process (`lowk-capture::SckSampler`), gated on Screen Recording preflight so it never prompts. Windows sampler is a stub (tone stays default) until the GDI sampler lands. `Tone.scrim_min` is applied in CSS as `max(k*.85, smin)`.
40. **Legibility**: overlay surfaces fade with the opacity slider but text alpha is clamped (`--tx >= .9`, `--tx2 >= .72`) and the contrast halo grows as the scrim fades.
41. **Header drag** in interactive mode is a plain pointer drag (`overlay_drag_start/to/end`); the hold-to-move magnet makes the window click-through, so it cannot be used with a mouse press.
42. **Dev server** URL is `http://127.0.0.1:1420` (Vite binds IPv4 only; WKWebView does not fall back from `localhost` -> `::1`).
43. **Swift runtime rpath**: `screencapturekit` links Swift libs via `@rpath`; link args of a dependency do not reach our binaries, so `lowk-capture/build.rs` and `lowk-app/build.rs` add `-rpath /usr/lib/swift`.
44. **`blake3` uses the `pure` feature**: the asm/MASM build needs `ml64.exe` and broke `cargo check --target x86_64-pc-windows-msvc` from macOS. Cross-check now needs only dummy `llvm-rc`.
45. **deny.toml** ignores three new Linux-only unmaintained advisories (zbus family via `keyring`).
46. **Updater** is a skeleton: `update_check` returns `enabled: false` until the minisign key exists; no plugin dependency yet. **Native folder picker** (`pickVaultDir`) needs `tauri-plugin-dialog` and is deferred; documents, index search and LLM summary stay on the mock.
47. **Probe hook** (`lowk-app/src/probe.rs`, debug builds only, active only with `LOWK_PROBE_CTL`/`LOWK_PROBE` env vars) lets `tools/stealth-probe/run-app.sh` drive the real windows (sentinel pages, scenarios) and exercises backend paths for verification. No code-eval hook exists.
