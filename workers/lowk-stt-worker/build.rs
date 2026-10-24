// Windows (MSVC): embed an application manifest with `longPathAware`, so the worker can open model files
// under paths longer than 260 characters (the app's own manifest does not apply to child processes).
fn main() {
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{dir}\\windows-app-manifest.xml");
    }
}
