//! OS probing: permissions, keychain secrets, hardware probe, locale, app dirs.
//! Everything here is synchronous; callers that must not block (Tauri commands)
//! wrap calls in `spawn_blocking`.

pub mod dirs;
pub mod hw;
pub mod locale;
pub mod perm;
pub mod secrets;
