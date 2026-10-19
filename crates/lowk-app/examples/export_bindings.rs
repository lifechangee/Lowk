//! `cargo run -p lowk-app --example export_bindings -- <out.ts>`
fn main() {
    let out = std::env::args().nth(1).expect("usage: export_bindings <out.ts>");
    lowk_app::export_bindings(std::path::Path::new(&out)).expect("export failed");
}
