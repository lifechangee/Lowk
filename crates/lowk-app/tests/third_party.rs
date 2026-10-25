//! THIRD_PARTY.md is generated from `tools/licenses/components.json`; the model rows there must match
//! what the app actually downloads (file, licence, licence link, source URL), and the file must ship.
use serde_json::Value;

fn components() -> Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/licenses/components.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
fn model_licences_match_the_download_registry() {
    // (file, licence, licence link, source URL)
    let mut specs: Vec<(String, String, String, String)> = vec![];
    for s in lowk_stt::local::nemotron_assets()
        .into_iter()
        .chain(lowk_stt::local::whisper_assets())
    {
        specs.push((s.file, s.license, s.license_url, s.url));
    }
    for s in lowk_index::embedder::local::gemma_assets() {
        specs.push((s.file, s.license, s.license_url, s.url));
    }
    let c = components();
    let models = c["models"].as_array().unwrap();
    assert_eq!(
        models.len(),
        specs.len(),
        "every downloadable model is listed, and only those"
    );
    for (file, license, license_url, url) in &specs {
        let m = models
            .iter()
            .find(|m| m["file"] == file.as_str())
            .unwrap_or_else(|| panic!("{file} missing in components.json"));
        assert_eq!(m["license"], license.as_str(), "{file}");
        assert_eq!(m["license_url"], license_url.as_str(), "{file}");
        assert_eq!(m["source_url"], url.as_str(), "{file}");
    }
}

#[test]
fn the_licence_file_ships_with_the_bundle_and_the_app() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let conf = std::fs::read_to_string(root.join("tauri.bundle.conf.json")).unwrap();
    assert!(
        conf.contains("THIRD_PARTY.md"),
        "bundle resources must include THIRD_PARTY.md"
    );
    assert!(root.join("../../THIRD_PARTY.md").is_file());
    assert!(lowk_app::about_licenses().contains("Downloadable models"));
}
