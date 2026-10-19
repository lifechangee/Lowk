//! Lowk input: `BindSpec` -> backends, `BindEvent{action, phase}`, cursor
//! source, conflict detection, record mode.
//!
//! * [`combo`]: adapter trait the app shell implements over
//!   `tauri-plugin-global-shortcut` (no permission).
//! * [`lowlevel`] + `platform`: own CGEventTap / WH_KEYBOARD_LL + WH_MOUSE_LL
//!   hooks on dedicated threads (hold binds, modifier-only, mouse side buttons,
//!   swallowing). Hooks never log key content.
//! * [`manager::InputManager`]: routes binds, merges events, builds `BindReport`.
pub mod combo;
pub mod conflicts;
pub mod cursor;
mod engine;
pub mod keys;
pub mod lowlevel;
pub mod manager;
pub mod matcher;
mod noop;
pub mod platform;

pub use combo::{ComboBackend, ComboEvents, ComboRegistrar};
pub use conflicts::{analyze as analyze_conflicts, soft_warning, Conflict, ConflictKind};
pub use cursor::SystemCursor;
pub use lowlevel::LowLevelBackend;
pub use manager::{InputHandle, InputManager, InputStatus};
pub use noop::NoopInputBackend;
pub use platform::{HookError, PermSnapshot};

use lowk_types::{BindSpec, Result};

/// Spec entry point: build a manager and apply `binds`.
pub fn start_input(
    binds: Vec<BindSpec>,
    registrar: Option<Box<dyn ComboRegistrar>>,
) -> Result<(InputHandle, lowk_types::BindReport)> {
    use lowk_types::InputBackend;
    let mut h = InputManager::new(registrar);
    let report = h.set_binds(&binds)?;
    Ok((h, report))
}
