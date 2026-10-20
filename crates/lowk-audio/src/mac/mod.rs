//! macOS backend: Core Audio process taps and HAL input (via `cidre`).
pub mod backend;
pub mod devices;
pub mod probe;
pub mod procs;
pub mod tap;

pub use backend::CoreAudio;
pub use probe::probe_system_audio;
