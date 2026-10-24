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

#[test]
fn s1_writer_never_stores_replacement_characters() {
    use lowk_store::*;
    use lowk_types::*;
    let d = tempfile::tempdir().unwrap();
    let v = Vault::open(d.path()).unwrap();
    let clock = SessionClock {
        t0_mono: 0,
        t0_wall_ms: 1_760_000_000_000,
    };
    let mut w = v
        .begin_call_with_clock(
            NewCall {
                title: "t".into(),
                folder: FolderRef::Unsorted,
                started: "2026-10-21T01:26:57Z".into(),
                sources: CallSources {
                    me: None,
                    system: None,
                },
                stt_engine: None,
                languages: vec![],
                speaker_me: "Я".into(),
                speaker_system: "Система".into(),
                clock: Some(clock),
            },
            clock,
        )
        .unwrap();
    w.push(CallEntry::Final {
        event: SttEvent {
            stream: Stream::Me,
            utt: 1,
            rev: 0,
            kind: SttKind::Final,
            text: "А, о\u{FFFD}кей. Расскажи ко\u{FFFD}".into(),
            lang: None,
            t_start_ns: 1_000_000_000,
            t_audio_end_ns: 2_000_000_000,
            t_emit_ns: 2_000_000_000,
        },
    })
    .unwrap();
    w.materialize().unwrap();
    let md = std::fs::read_to_string(d.path().join(w.rel_path())).unwrap();
    assert!(!md.contains('\u{FFFD}'), "{md}");
    assert!(md.contains("А, окей. Расскажи ко"));
}

#[test]
fn s6_lines_are_ordered_by_audio_time_not_by_final_arrival() {
    use lowk_store::*;
    use lowk_types::*;
    let d = tempfile::tempdir().unwrap();
    let v = Vault::open(d.path()).unwrap();
    let clock = SessionClock {
        t0_mono: 5_000_000_000,
        t0_wall_ms: 1_760_000_000_000,
    };
    let mut w = v
        .begin_call_with_clock(
            NewCall {
                title: "t".into(),
                folder: FolderRef::Unsorted,
                started: "2026-10-21T01:26:57Z".into(),
                sources: CallSources {
                    me: None,
                    system: None,
                },
                stt_engine: None,
                languages: vec![],
                speaker_me: "Я".into(),
                speaker_system: "Система".into(),
                clock: Some(clock),
            },
            clock,
        )
        .unwrap();
    let fin = |stream, utt, a: u64, b: u64, text: &str| CallEntry::Final {
        event: SttEvent {
            stream,
            utt,
            rev: 0,
            kind: SttKind::Final,
            text: text.into(),
            lang: None,
            // audio time relative to the call start, on the shared monotonic clock
            t_start_ns: 5_000_000_000 + a * 1_000_000_000,
            t_audio_end_ns: 5_000_000_000 + b * 1_000_000_000,
            t_emit_ns: 0,
        },
    };
    // arrival order: System 40 s, Q&A asked at 50 s, then the Me final that started at 20 s
    w.push(fin(Stream::System, 1, 40, 44, "Вопрос про метрики."))
        .unwrap();
    w.push(CallEntry::Qa {
        id: Ulid::new(),
        at_s: 50,
        template: "brief".into(),
        model: "m".into(),
        range: "00:00:00-00:00:50".into(),
        question: "q".into(),
        answer: "a".into(),
    })
    .unwrap();
    w.push(fin(Stream::Me, 1, 20, 30, "Мой ответ раньше.")).unwrap();
    w.push(fin(Stream::Me, 2, 0, 3, "Самая первая реплика.")).unwrap();
    w.materialize().unwrap();
    let md = std::fs::read_to_string(d.path().join(w.rel_path())).unwrap();
    let pos = |needle: &str| {
        md.find(needle)
            .unwrap_or_else(|| panic!("{needle} missing in\n{md}"))
    };
    assert!(md.contains("[00:00:00] **Я:** Самая первая реплика."));
    assert!(md.contains("[00:00:20] **Я:** Мой ответ раньше."));
    assert!(md.contains("[00:00:40] **Система:** Вопрос про метрики."));
    assert!(
        pos("Самая первая") < pos("Мой ответ раньше") && pos("Мой ответ раньше") < pos("Вопрос про метрики")
    );
    assert!(pos("Вопрос про метрики") < pos("lowk-qa"));
}

#[test]
fn mic_marker_line_roundtrips_and_is_not_a_speaker_turn() {
    let src = "---\nlowk: 1\ntype: call\nid: X\ntitle: \"t\"\n---\n\n# t\n\n## Транскрипт\n\n[00:00:05] **Система:** привет\n\n[00:00:12] *Микрофон выключен*\n\n[00:01:30] *Микрофон включён*\n\n[00:01:41] **Я:** снова тут\n";
    let doc = call_codec::parse(src);
    let blocks = doc.blocks();
    assert!(
        matches!(&blocks[1], Block::Marker { at_s: 12, text } if text == "Микрофон выключен"),
        "{blocks:?}"
    );
    assert!(matches!(&blocks[2], Block::Marker { at_s: 90, text } if text == "Микрофон включён"));
    assert!(matches!(&blocks[3], Block::Turn { .. }));
    let out = call_codec::render(&doc);
    assert!(out.ends_with(&src[src.find("## Транскрипт").unwrap()..]), "{out}");
}

#[test]
fn folder_mic_preset_roundtrips_and_inherit_writes_nothing() {
    use lowk_types::MicPreset;
    let mut doc = folder_codec::parse(&fixture("folder_full.md"));
    assert_eq!(doc.mic, MicPreset::Inherit, "no key = inherit");
    assert!(!folder_codec::render(&doc).contains("record_mic"));
    for (preset, text) in [
        (MicPreset::Off, "record_mic: off"),
        (MicPreset::On, "record_mic: on"),
    ] {
        doc.mic = preset;
        let md = folder_codec::render(&doc);
        assert!(md.contains(text), "{md}");
        assert_eq!(folder_codec::parse(&md).mic, preset);
    }
}
