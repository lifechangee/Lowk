# lowk-input: proposed contract changes (lowk-types)

1. `Phase`: the brief mentions `Pressed/Held/Released`; the contract has `Down/Up`. We implemented `Down/Up` only (a hold = time between them). Auto-repeat is suppressed. A `Held` phase is not needed; no change proposed.
2. `PermKind::InputMonitoring` / `Accessibility` already cover `BindStatus::Degraded`; no change. Suggest adding `BindStatus::Degraded` detail text is unnecessary.
3. Optional: add `InputStatus` (see `lowk_input::InputStatus`: combo adapter present, hook running, can swallow, permissions, binds working without hook) to `lowk-types` if the UI wants it over IPC for `binds_*` commands. Currently Rust-only (not specta).
4. Optional: `KeyCode` validation helper (`lowk_input::keys::is_known_code`) could move to `lowk-types` so `Settings::apply_patch` rejects unknown codes.
5. `MouseBtn::Other(n)`: n is the macOS button number (>= 5); unsupported on Windows (reported `Failed`).

Semantics: duplicate triggers/actions => later bind `Conflict("duplicate of X")` and is not registered; OS-reserved shortcut => registered but `Conflict(name)`. Soft warnings (Cmd+Enter, Cmd+Shift+S) via `lowk_input::soft_warning`.
