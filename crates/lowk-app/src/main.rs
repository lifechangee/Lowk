// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Headless diagnostics of the M2 pipeline (docs, index, Ask flow); used by `just qa-bundle`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--selfcheck-m2") {
        std::process::exit(lowk_app::m2::selfcheck::main(&args));
    }
    if args.iter().any(|a| a == "--selfcheck-audio") {
        std::process::exit(lowk_app::audio_selfcheck::main(&args));
    }
    if args.iter().any(|a| a == "--selfcheck-soak") {
        std::process::exit(lowk_app::soak::main(&args));
    }
    lowk_app::run();
}
