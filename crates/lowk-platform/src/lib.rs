//! OS probing: permissions, keychain secrets, hardware probe, locale, app dirs.
//! WP5 owns this crate; signatures follow `docs/ARCHITECTURE.md` section 10.
//! Everything here is synchronous; callers that must not block (Tauri commands)
//! wrap calls in `spawn_blocking`.

pub mod dirs;
pub mod hw;
pub mod locale;
pub mod perm;
pub mod secrets;
