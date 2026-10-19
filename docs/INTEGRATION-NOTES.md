# M0 integration notes (for WP6 app shell)

- WP9 overlay calls commands by guessed names without bindings: `llm_ask`, `overlay_input_submit`, `llm_set_template`, `capture_screenshot`, `session_toggle_record`. Align with ARCHITECTURE §10 (`session_start`/`session_stop` — no `session_toggle_record`) and regenerate bindings.
- WP9: at 90% transparency on light background text is faint — keep text legible (clamp text/fg alpha separately from surfaces or raise contrast with adaptive shadow).
- WP9/WP8: merge `ui/src/*/i18n-keys.todo.json` into lib/i18n and `TOKENS-NEEDED.md` into tokens.css.
- WP5: proposes `HwReport`, `SttTier`, `Recommendation` into lowk-types (crates/lowk-platform/CONTRACT-CHANGES.md).
