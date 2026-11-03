//! Library management: rename calls / folders / documents, trashed documents, moving documents.
use lowk_store::*;
use lowk_types::*;
use std::fs;

fn open() -> (tempfile::TempDir, Vault) {
    let d = tempfile::tempdir().unwrap();
    let v = Vault::open(&d.path().join("vault")).unwrap();
    (d, v)
}

fn clock() -> SessionClock {
    SessionClock {
        t0_mono: 0,
        t0_wall_ms: 0,
    }
}

fn new_call(folder: FolderRef, title: &str) -> NewCall {
    NewCall {
        title: title.into(),
        folder,
        started: "2026-10-19T10:30:12+03:00".into(),
        sources: CallSources {
            me: Some("Mic".into()),
            system: Some("Zoom".into()),
        },
        stt_engine: Some("fake".into()),
        languages: vec![],
        speaker_me: "Я".into(),
        speaker_system: "Система".into(),
        clock: None,
    }
}

fn final_entry(sec: u64, text: &str) -> CallEntry {
    let ns = sec * 1_000_000_000;
    CallEntry::Final {
        event: SttEvent {
            stream: Stream::Me,
            utt: sec,
            rev: 1,
            kind: SttKind::Final,
            text: text.into(),
            lang: Some("ru".into()),
            t_start_ns: ns,
            t_audio_end_ns: ns + 1_000_000_000,
            t_emit_ns: ns,
        },
    }
}

fn record(v: &Vault, folder: FolderRef, title: &str) -> CallMeta {
    let mut w = v.begin_call_with_clock(new_call(folder, title), clock()).unwrap();
    w.push(final_entry(3, "привет")).unwrap();
    w.finish().unwrap()
}

fn folder(v: &Vault, title: &str) -> (Ulid, FolderRef) {
    let m = v
        .create_folder(NewFolder {
            title: title.into(),
            allow_cloud: true,
            instructions: String::new(),
        })
        .unwrap();
    let FolderRef::User(id) = m.id.clone() else {
        panic!()
    };
    (id, m.id)
}

fn text_of(v: &Vault, m: &CallMeta) -> String {
    fs::read_to_string(v.root().join(&m.path)).unwrap()
}

#[test]
fn rename_call_changes_front_matter_heading_and_file_name() {
    let (_t, v) = open();
    let m = record(&v, FolderRef::Unsorted, "Old name");
    assert!(m.path.ends_with("Old name.md"), "{}", m.path);
    let r = v.rename_call(&m.id, "  New name  ").unwrap();
    assert_eq!(r.title, "New name");
    assert!(r.path.ends_with("2026-10-19 1030 New name.md"), "{}", r.path);
    assert!(!v.root().join(&m.path).exists());
    let t = text_of(&v, &r);
    assert!(t.contains("title: New name") && t.contains("# New name"), "{t}");
    assert_eq!(v.calls(None).unwrap().len(), 1);
}

#[test]
fn rename_call_refuses_empty_and_allows_duplicates() {
    let (_t, v) = open();
    let a = record(&v, FolderRef::Unsorted, "A");
    let b = record(&v, FolderRef::Unsorted, "B");
    assert_eq!(
        v.rename_call(&a.id, "   ").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        v.calls(None)
            .unwrap()
            .iter()
            .find(|c| c.id == a.id)
            .unwrap()
            .title,
        "A"
    );
    let x = v.rename_call(&a.id, "Same").unwrap();
    let y = v.rename_call(&b.id, "Same").unwrap();
    assert_eq!((x.title.as_str(), y.title.as_str()), ("Same", "Same"));
    assert_ne!(x.path, y.path, "two calls with one title keep two files");
}

#[test]
fn rename_call_keeps_its_assets_embeds_working() {
    let (_t, v) = open();
    let mut w = v
        .begin_call_with_clock(new_call(FolderRef::Unsorted, "Shots"), clock())
        .unwrap();
    let link = w.save_asset("a.png", b"png").unwrap();
    assert!(link.contains(".assets/"));
    let m = w.finish().unwrap();
    let r = v.rename_call(&m.id, "Renamed").unwrap();
    let stem = r.path.rsplit('/').next().unwrap().trim_end_matches(".md");
    assert!(v
        .root()
        .join(&r.path)
        .with_file_name(format!("{stem}.assets"))
        .join("a.png")
        .exists());
}

#[test]
fn rename_while_recording_shows_now_and_renames_the_file_after_stop() {
    let (_t, v) = open();
    let mut w = v
        .begin_call_with_clock(new_call(FolderRef::Unsorted, "Live"), clock())
        .unwrap();
    w.push(final_entry(3, "раз")).unwrap();
    let path_before = w.rel_path();
    let r = v.rename_call(&w.id(), "Renamed live").unwrap();
    assert_eq!(r.title, "Renamed live");
    assert_eq!(
        r.path, path_before,
        "the file keeps its name while the call is recorded"
    );
    assert!(r.ended.is_none());
    // the writer's next write neither loses the title nor reports an outside edit
    w.push(final_entry(6, "два")).unwrap();
    w.materialize().unwrap();
    assert!(w.take_notice().is_none());
    let live = v
        .calls(None)
        .unwrap()
        .into_iter()
        .find(|c| c.id == w.id())
        .unwrap();
    assert_eq!(live.title, "Renamed live");
    let done = w.finish().unwrap();
    assert_eq!(done.title, "Renamed live");
    assert!(
        done.path.ends_with("2026-10-19 1030 Renamed live.md"),
        "{}",
        done.path
    );
    let t = text_of(&v, &done);
    assert!(
        t.contains("title: Renamed live")
            && t.contains("# Renamed live")
            && t.contains("раз")
            && t.contains("два")
    );
    assert!(!v.root().join(&path_before).exists());
}

#[test]
fn rename_folder_updates_directory_card_and_calls() {
    let (_t, v) = open();
    let (id, f) = folder(&v, "Old folder");
    let c = record(&v, f.clone(), "Call");
    let m = v
        .update_folder(
            &id,
            FolderPatch {
                title: Some(" New folder ".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(m.title, "New folder");
    assert!(m.path.contains("New folder"), "{}", m.path);
    assert!(!v.root().join("Old folder").exists());
    let calls = v.calls_in(&f).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, c.id);
    assert!(text_of(&v, &calls[0]).contains("folder: New folder"));
    // an empty title changes nothing
    let same = v
        .update_folder(
            &id,
            FolderPatch {
                title: Some("  ".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(same.title, "New folder");
}

#[test]
fn rename_folder_is_refused_while_a_call_is_recorded_in_it() {
    let (_t, v) = open();
    let (id, f) = folder(&v, "Busy");
    let w = v.begin_call_with_clock(new_call(f, "Live"), clock()).unwrap();
    let e = v
        .update_folder(
            &id,
            FolderPatch {
                title: Some("Other".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::SessionBusy);
    assert!(v.root().join("Busy").exists());
    drop(w);
}

#[test]
fn rename_document_changes_the_title_only() {
    let (t, v) = open();
    let (_id, f) = folder(&v, "P");
    let p = t.path().join("a.txt");
    fs::write(&p, b"hello").unwrap();
    let d = v
        .add_doc(&NewDocument {
            folder: f,
            source_path: p.to_string_lossy().into(),
            title: None,
            pin: false,
            on_duplicate: Default::default(),
        })
        .unwrap();
    let r = v
        .patch_doc(
            &d.id,
            &DocPatch {
                title: Some(" Renamed ".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(r.title, "Renamed");
    assert_eq!(r.md_path, d.md_path, "file names stay stable");
    assert_eq!(r.original_path, d.original_path);
    assert_eq!(fs::read(v.root().join(&r.original_path)).unwrap(), b"hello");
    assert_eq!(v.get_doc(&d.id).unwrap().title, "Renamed");
    let e = v
        .patch_doc(
            &d.id,
            &DocPatch {
                title: Some("  ".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
    assert_eq!(v.get_doc(&d.id).unwrap().title, "Renamed");
}

#[test]
fn trashed_documents_list_alone_and_restore_but_folder_contents_do_not() {
    let (t, v) = open();
    let (fid, f) = folder(&v, "P");
    let p = t.path().join("a.txt");
    fs::write(&p, b"hello").unwrap();
    let mk = |name: &str| {
        let q = t.path().join(name);
        fs::write(&q, name.as_bytes()).unwrap();
        v.add_doc(&NewDocument {
            folder: f.clone(),
            source_path: q.to_string_lossy().into(),
            title: None,
            pin: false,
            on_duplicate: Default::default(),
        })
        .unwrap()
    };
    let a = mk("a.txt");
    let b = mk("b.txt");
    v.delete_doc(&a.id).unwrap();
    let trash = v.list_docs(&FolderRef::Trash).unwrap();
    assert_eq!(trash.iter().map(|d| d.id).collect::<Vec<_>>(), vec![a.id]);
    // a document inside a trashed folder is listed with its folder, not on its own
    v.delete(&fid).unwrap();
    let trash = v.list_docs(&FolderRef::Trash).unwrap();
    assert_eq!(
        trash.len(),
        1,
        "{:?}",
        trash.iter().map(|d| &d.title).collect::<Vec<_>>()
    );
    assert_ne!(trash[0].id, b.id);
    let back = v.restore_doc(&a.id).unwrap();
    assert!(!back.deleted);
}

#[test]
fn move_document_between_folders_and_back_keeps_title_and_original() {
    let (t, v) = open();
    let (_a, fa) = folder(&v, "A");
    let (_b, fb) = folder(&v, "B");
    let p = t.path().join("x.txt");
    fs::write(&p, b"hello").unwrap();
    let d = v
        .add_doc(&NewDocument {
            folder: fa.clone(),
            source_path: p.to_string_lossy().into(),
            title: Some("Мой документ".into()),
            pin: false,
            on_duplicate: Default::default(),
        })
        .unwrap();
    let moved = v
        .patch_doc(
            &d.id,
            &DocPatch {
                folder: Some(fb.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(moved.folder, fb);
    assert_eq!(moved.title, "Мой документ");
    assert!(v.list_docs(&fa).unwrap().is_empty());
    assert_eq!(v.list_docs(&fb).unwrap().len(), 1);
    assert_eq!(fs::read(v.root().join(&moved.original_path)).unwrap(), b"hello");
    // to Unsorted too
    let u = v
        .patch_doc(
            &d.id,
            &DocPatch {
                folder: Some(FolderRef::Unsorted),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(u.folder, FolderRef::Unsorted);
}
