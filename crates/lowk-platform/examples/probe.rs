//! Prints hardware probe, recommendation, locale and permission statuses.
//! Never prompts and never changes settings.
//! `cargo run -p lowk-platform --example probe`
use lowk_platform::{dirs, hw, locale, perm};

fn main() {
    let r = hw::probe_report();
    println!("{r:#?}");
    println!("recommendation: {:?}", hw::recommend(&r));
    println!("locale: {} -> ui {}", locale::system(), locale::ui_lang());
    println!("app data: {:?}", dirs::app_data());
    for k in perm::ALL {
        println!("perm {k:?}: {:?}", perm::status(k));
    }
}
