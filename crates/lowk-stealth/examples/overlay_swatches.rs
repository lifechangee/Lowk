//! Writes docs/design/overlay-color-swatches.{json,html} from the real adaptive tone:
//! `cargo run -p lowk-stealth --features swatches --example overlay_swatches`
//! (the PNG is rendered from the HTML by `pnpm --dir ui exec playwright test overlay-colors`, SWATCH_PNG=1).
use lowk_stealth::swatches::{run_all, to_html, to_json};

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design");
    let rs = run_all();
    std::fs::write(dir.join("overlay-color-swatches.json"), to_json(&rs)).unwrap();
    std::fs::write(dir.join("overlay-color-swatches.html"), to_html(&rs)).unwrap();
    let bad: Vec<_> = rs.iter().filter(|r| !r.ok()).map(|r| r.name).collect();
    println!("{} swatches, {} failing {:?}", rs.len(), bad.len(), bad);
}
