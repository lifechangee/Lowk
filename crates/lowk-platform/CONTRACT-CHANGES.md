# lowk-platform contract changes (proposals)

Existing signatures (`perm::{status,request,open_settings}`, `secrets::*`, `hw::probe() -> HwInfo`, `locale::system()`) are unchanged.

Additions in this crate (no lowk-types edits made):

1. `hw::probe_report() -> HwReport { info: HwInfo, physical_cores, logical_cores, apple_silicon_gen }` and `hw::recommend(&HwReport) -> Recommendation { tier: SttTier{LocalHigh,LocalLight,Cloud}, reason: RecReason }`. Proposal: move `SttTier`, `RecReason`, `Recommendation` (and optionally `physical_cores`/`logical_cores` into `HwInfo`) to `lowk-types` with `specta::Type` so `hw_probe` can ship them to the UI. `HwInfo` lacks core counts, hence `HwReport`.
2. `perm::report_system_audio_probe(SystemAudioProbe::{Working,Failed,Silent})`: lowk-audio must call it after each probe tap; `status(SystemAudio)` derives from it (macOS has no public status API). `request(SystemAudio)` cannot prompt; the prompt comes from creating the tap.
3. `perm::request(Microphone)` blocks until the user answers (up to 5 min): call from `spawn_blocking`.
4. `perm::request(Accessibility)` opens the pane (no prompt). `status` for ScreenRecording / Accessibility returns `NotDetermined` instead of `Denied` when not granted (no tri-state API).
5. `locale::ui_lang() -> "ru" | "en"`, `locale::resolve(tag)`, `dirs::{app_data, models, engines, logs, default_vault}`, `perm::{ALL, settings_url}`.
6. `secrets::present` reads the item (may trigger a Keychain ACL prompt on unsigned builds).
