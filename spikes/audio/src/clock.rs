//! Monotonic ns clock that matches Core Audio host time on macOS.
#[cfg(target_os = "macos")]
pub fn now_ns() -> u64 {
    cidre::core_audio::host_time_to_nanos(cidre::core_audio::current_host_time())
}
#[cfg(not(target_os = "macos"))]
pub fn now_ns() -> u64 {
    use std::sync::OnceLock;
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64
}
