use lowk_store::call_codec::{self, Block, Speaker};
use lowk_store::folder_codec;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn golden_call_roundtrip_is_byte_exact() {
    for f in ["call_full.md", "call_minimal.md"] {
        let src = fixture(f);
        let doc = call_codec::parse(&src);
        assert_eq!(call_codec::render(&doc), src, "{f}");
    }
}

#[test]
fn golden_folder_roundtrip_is_byte_exact() {
    let src = fixture("folder_full.md");
    let doc = folder_codec::parse(&src);
    assert_eq!(folder_codec::render(&doc), src);
    assert_eq!(doc.tags, vec!["client", "a, b"]);
    assert!(!doc.allow_cloud);
    assert_eq!(doc.docs(), ["Docs/spec", "Docs/pricing"]);
    assert!(doc.instructions().starts_with("Короткие"));
}

#[test]
fn call_fields_are_parsed() {
    let doc = call_codec::parse(&fixture("call_full.md"));
    assert_eq!(doc.fm.id, "01JC8Z3K6Q9X2M4N7R5T0V8WBD");
    assert_eq!(doc.fm.languages, vec!["ru", "en"]);
    assert_eq!(doc.fm.source_system.as_deref(), Some("Zoom"));
    assert_eq!(doc.fm.duration_s, Some(4363));
    assert!(doc.summary().contains("EBITDA"));
    let blocks = doc.blocks();
    assert!(matches!(
        &blocks[0],
        Block::Turn {
            at_s: 3,
            speaker: Speaker::Me,
            ..
        }
    ));
    match &blocks[1] {
        Block::Turn {
            speaker: Speaker::System,
            text,
            ..
        } => assert!(text.contains('\n')),
        b => panic!("{b:?}"),
    }
    let qa = blocks
        .iter()
        .find_map(|b| if let Block::Qa(q) = b { Some(q) } else { None })
        .unwrap();
    assert_eq!(qa.at_s, 24);
    assert_eq!(qa.model, "deepseek/deepseek-v4.1-flash");
    assert_eq!(qa.my_answer.as_ref().unwrap().0, 31);
    assert!(qa.answer.contains("Второй абзац"));
    assert!(matches!(
        blocks.iter().rev().nth(1),
        Some(Block::Raw(_)) | Some(Block::Turn { .. })
    ));
}

#[test]
fn unknown_frontmatter_and_sections_survive_edits() {
    let mut doc = call_codec::parse(&fixture("call_full.md"));
    doc.fm.tags.push("new".into());
    let out = call_codec::render(&doc);
    assert!(out.contains("custom_key: keep me"));
    assert!(out.contains("nested:\n  - a\n  - b"));
    assert!(out.contains("## Notes\n\nMy own notes, preserved."));
    assert!(out.contains("tags: [sales, weekly, new]"));
}

#[test]
fn hand_edited_crlf_and_comments_are_tolerated() {
    let src = "---\r\nlowk: 1\r\ntype: call\r\nid: 01JC8Z3K6Q9X2M4N7R5T0V8WBE\r\ntitle: T\r\naudio: none   # none | kept\r\n---\r\n\r\n# T\r\n\r\n## Transcript\r\n\r\n[00:00:01] **Я:** a\r\n[00:00:02] **Незнакомец:** b\r\n";
    let doc = call_codec::parse(src);
    assert_eq!(doc.fm.audio, "none");
    let again = call_codec::parse(&call_codec::render(&doc));
    assert_eq!(again.blocks(), doc.blocks());
    assert!(matches!(&doc.blocks()[1], Block::Turn { speaker: Speaker::Other(l), .. } if l == "Незнакомец"));
}

#[test]
fn garbage_never_panics() {
    for s in [
        "",
        "---",
        "---\n---\n",
        "no frontmatter\n\n## Transcript\n[x",
        "---\nid\n---\n\n## Transcript\n> [!lowk-qa]",
    ] {
        let d = call_codec::parse(s);
        let _ = call_codec::render(&d);
        let _ = folder_codec::render(&folder_codec::parse(s));
    }
}
