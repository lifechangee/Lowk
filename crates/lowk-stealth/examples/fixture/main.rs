//! Stealth fixture for `tools/stealth-probe`; implemented for macOS (see `mac.rs`).
#[cfg(target_os = "macos")]
mod mac;

fn main() {
    #[cfg(target_os = "macos")]
    mac::main();
    #[cfg(not(target_os = "macos"))]
    eprintln!("the stealth fixture is only implemented for macOS");
}
