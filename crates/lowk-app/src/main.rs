// GUI subsystem in every profile: no console window on Windows (debug builds included).
#![windows_subsystem = "windows"]

fn main() {
    // Headless diagnostics of the documents pipeline (docs, index, Ask flow); used by `just qa-bundle`.
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
