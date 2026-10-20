//! `screencapturekit` links Swift runtime libraries through `@rpath`; link
//! arguments of a dependency's build script do not reach our test binaries, so
//! the rpath is repeated here (and in `lowk-app/build.rs` for the app).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
