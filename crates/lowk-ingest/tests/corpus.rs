//! Corpus harness: every file listed in fixtures/ingest/MANIFEST.tsv either converts with the
//! expected page count and key phrases, or fails with exactly the expected error. No file may end
//! up as a silent empty document. Run: `cargo test -p lowk-ingest --features corpus`.
#![cfg(feature = "corpus")]
use lowk_ingest::*;
use lowk_types::*;
use std::path::Path;

#[test]
fn corpus_matches_manifest() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ingest");
    let manifest = std::fs::read_to_string(dir.join("MANIFEST.tsv")).expect("manifest");
    let ing = Ingestor::new();
    let mut checked = 0;
    let mut problems = vec![];
    for (n, line) in manifest.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let c: Vec<&str> = line.split('\t').collect();
        assert!(
            c.len() >= 3,
            "manifest line {}: expected at least 3 columns",
            n + 1
        );
        let (file, expect, pages, phrases) = (c[0], c[1], c[2], c.get(3).copied().unwrap_or(""));
        let flags = c.get(4).copied().unwrap_or("");
        if flags.contains("ocr") && !ing.ocr_available() {
            eprintln!("skip {file}: no OCR engine on this platform");
            continue;
        }
        let path = dir.join(file);
        let ext = path.extension().unwrap().to_string_lossy().to_string();
        let kind = DocKind::from_extension(&ext).unwrap_or_else(|| panic!("{file}: unknown extension"));
        let req = IngestRequest {
            doc: Ulid::new(),
            source_path: path.to_string_lossy().into(),
            kind,
            languages: vec!["ru".into(), "en".into()],
            ocr: true,
        };
        checked += 1;
        let res = ing.convert(&req, &mut |_, _| {}, &|| true);
        match (expect, res) {
            ("ok", Ok(d)) => {
                if pages != "-" && d.pages != pages.parse().ok() {
                    problems.push(format!("{file}: pages {:?}, expected {pages}", d.pages));
                }
                for ph in phrases.split(";;").filter(|p| !p.is_empty()) {
                    if !d.md.contains(ph) {
                        problems.push(format!("{file}: missing phrase {ph:?}"));
                    }
                }
                if d.md.chars().filter(|c| c.is_alphanumeric()).count() == 0 {
                    problems.push(format!("{file}: silent empty document"));
                }
                if d.pages.is_some()
                    && kind != DocKind::Docx
                    && markers(&d.md) != (1..=d.pages.unwrap()).collect::<Vec<_>>()
                {
                    problems.push(format!("{file}: page markers do not match the page count"));
                }
            }
            ("ok", Err(e)) => problems.push(format!("{file}: expected ok, got {e}")),
            (code, Ok(_)) => problems.push(format!("{file}: expected error {code}, got a document")),
            (code, Err(e)) => {
                if format!("{:?}", e.code) != code {
                    problems.push(format!("{file}: expected error {code}, got {:?}", e.code));
                }
            }
        }
    }
    assert!(checked > 0);
    assert!(problems.is_empty(), "corpus problems:\n{}", problems.join("\n"));
}

fn markers(md: &str) -> Vec<u32> {
    md.lines()
        .filter_map(|l| l.strip_prefix("<!-- p:")?.strip_suffix(" -->")?.parse().ok())
        .collect()
}
