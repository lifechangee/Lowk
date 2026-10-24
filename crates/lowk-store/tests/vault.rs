use lowk_store::*;
use lowk_types::*;
use std::fs;
use std::time::{Duration, Instant};

fn new_call(folder: FolderRef) -> NewCall {
    NewCall {
        title: "Weekly sync".into(),
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

fn fin(stream: Stream, sec: u64, text: &str) -> CallEntry {
    let ns = sec * 1_000_000_000;
    CallEntry::Final {
        event: SttEvent {
            stream,
            utt: sec,
            rev: 1,
            kind: SttKind::Final,
            text: text.into(),
            lang: Some("ru".into()),
            t_start_ns: ns,
            t_audio_end_ns: ns + 2_000_000_000,
            t_emit_ns: ns,
        },
    }
}

fn clock() -> SessionClock {
    SessionClock {
        t0_mono: 0,
        t0_wall_ms: 0,
    }
}

fn open() -> (tempfile::TempDir, Vault) {
    let d = tempfile::tempdir().unwrap();
    let v = Vault::open(d.path()).unwrap();
    (d, v)
}

fn record(v: &Vault, folder: FolderRef) -> CallMeta {
    let mut w = v.begin_call_with_clock(new_call(folder), clock()).unwrap();
    w.push(fin(Stream::Me, 3, "Привет")).unwrap();
    w.push(fin(Stream::Me, 6, "слышно меня?")).unwrap();
    w.push(fin(Stream::System, 9, "Да")).unwrap();
    let qa = Ulid::new();
    w.push(CallEntry::Qa {
        id: qa,
        at_s: 12,
        template: "brief".into(),
        model: "m".into(),
        range: "00:00:03-00:00:11".into(),
        question: "Q?".into(),
        answer: "A.".into(),
    })
    .unwrap();
    w.push(CallEntry::MyAnswer {
        qa_id: qa,
        at_s: 15,
        text: "мой".into(),
    })
    .unwrap();
    w.push(CallEntry::MyAnswer {
        qa_id: qa,
        at_s: 17,
        text: "ответ".into(),
    })
    .unwrap();
    w.finish_at("2026-10-19T10:31:12+03:00".into()).unwrap()
}

#[test]
fn open_creates_layout_and_system_folders() {
    let (d, v) = open();
    assert!(d.path().join("Unsorted/Calls").is_dir());
    assert!(d.path().join(".lowk/trash").is_dir());
    let f = v.folders().unwrap();
    assert!(f.iter().any(|m| m.id == FolderRef::Unsorted));
    assert!(f.iter().any(|m| m.id == FolderRef::Trash));
}

#[test]
fn folder_and_call_lifecycle() {
    let (d, v) = open();
    let fm = v
        .create_folder(NewFolder {
            title: "Клиент Альфа".into(),
            tags: vec!["client".into()],
            allow_cloud: true,
            instructions: "be brief".into(),
        })
        .unwrap();
    let FolderRef::User(fid) = fm.id.clone() else {
        panic!()
    };
    assert!(d.path().join("Клиент Альфа/_folder.md").exists());
    assert_eq!(v.folder_doc(&fid).unwrap().instructions(), "be brief");

    let meta = record(&v, fm.id.clone());
    assert_eq!(meta.duration_s, Some(60));
    assert_eq!(meta.languages, vec!["ru"]);
    assert!(meta
        .path
        .starts_with("Клиент Альфа/Calls/2026-10-19 1030 Weekly sync"));
    assert_eq!(v.calls(Some(&fid)).unwrap().len(), 1);
    assert_eq!(
        v.folders()
            .unwrap()
            .iter()
            .find(|f| f.id == fm.id)
            .unwrap()
            .call_count,
        1
    );

    let md = v.read_call(&meta.id).unwrap().md;
    assert!(md.contains("[00:00:03] **Я:** Привет слышно меня?"), "{md}");
    assert!(md.contains("[00:00:09] **Система:** Да"));
    assert!(md.contains("> **Мой ответ:** [00:00:15] мой ответ"));
    // journal removed after finish
    assert!(v.recoverable_calls().unwrap().is_empty());

    // editor save + title edit renames the file
    let edited = md
        .replace("title: Weekly sync", "title: Renamed")
        .replace("# Weekly sync", "# Renamed");
    v.save_md(&meta.id, &edited).unwrap();
    let m2 = v.read_call(&meta.id).unwrap().meta;
    assert!(m2.path.ends_with("2026-10-19 1030 Renamed.md"), "{}", m2.path);
    // wrong id is rejected
    assert!(v
        .save_md(
            &meta.id,
            &edited.replace(&meta.id.to_string(), &Ulid::new().to_string())
        )
        .is_err());

    v.set_call_tags(&meta.id, vec!["a".into()]).unwrap();
    assert_eq!(v.read_call(&meta.id).unwrap().meta.tags, vec!["a"]);

    let moved = v.move_call(&meta.id, &FolderRef::Unsorted).unwrap();
    assert!(moved.path.starts_with("Unsorted/Calls/"));
    assert_eq!(v.calls(Some(&fid)).unwrap().len(), 0);

    let up = v
        .update_folder(
            &fid,
            FolderPatch {
                title: Some("Alpha".into()),
                instructions: Some("x".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(up.path, "Alpha");
    assert!(d.path().join("Alpha/_folder.md").exists());
}

#[test]
fn trash_restore_and_empty() {
    let (d, v) = open();
    let meta = record(&v, FolderRef::Unsorted);
    let assets = d.path().join("Unsorted/Calls/2026-10-19 1030 Weekly sync.assets");
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("a.png"), b"png").unwrap();

    v.delete(&meta.id).unwrap();
    assert!(v.calls(None).unwrap().is_empty());
    let trashed = v.calls_in(&FolderRef::Trash).unwrap();
    assert_eq!(trashed.len(), 1);
    assert!(trashed[0].deleted);
    assert!(!d
        .path()
        .join("Unsorted/Calls/2026-10-19 1030 Weekly sync.md")
        .exists());
    assert!(v.read_call(&meta.id).is_ok(), "trashed calls stay readable");
    assert_eq!(v.trash_items().unwrap().len(), 1);
    assert!(v.save_md(&meta.id, "x").is_err());

    // name collision on restore -> suffix, assets follow
    fs::write(
        d.path().join("Unsorted/Calls/2026-10-19 1030 Weekly sync.md"),
        "occupied",
    )
    .unwrap();
    v.restore(&meta.id).unwrap();
    assert!(v.trash_items().unwrap().is_empty());
    let back = v.read_call(&meta.id).unwrap().meta;
    assert!(back.path.ends_with("Weekly sync (2).md"), "{}", back.path);
    assert!(d
        .path()
        .join("Unsorted/Calls/2026-10-19 1030 Weekly sync (2).assets/a.png")
        .exists());
    assert!(!back.deleted);

    // nothing is hard-deleted until empty_trash
    v.delete(&meta.id).unwrap();
    assert!(d.path().join(".lowk/trash").join(meta.id.to_string()).exists());
    assert_eq!(v.empty_trash().unwrap(), 1);
    assert!(v.read_call(&meta.id).is_err());
    assert!(v.trash_items().unwrap().is_empty());
}

#[test]
fn trash_folder_with_calls_and_restore() {
    let (_d, v) = open();
    let fm = v
        .create_folder(NewFolder {
            title: "P".into(),
            tags: vec![],
            allow_cloud: true,
            instructions: String::new(),
        })
        .unwrap();
    let FolderRef::User(fid) = fm.id.clone() else {
        panic!()
    };
    let meta = record(&v, fm.id);
    v.delete(&fid).unwrap();
    assert!(v.folders().unwrap().iter().all(|f| f.id != FolderRef::User(fid)));
    assert!(v
        .calls_in(&FolderRef::Trash)
        .unwrap()
        .iter()
        .any(|c| c.id == meta.id));
    v.restore(&fid).unwrap();
    assert_eq!(v.calls(Some(&fid)).unwrap().len(), 1);
}

#[test]
fn journal_recovery_after_crash() {
    let (d, v) = open();
    let mut w = v
        .begin_call_with_clock(new_call(FolderRef::Unsorted), clock())
        .unwrap();
    let id = w.id();
    w.push(fin(Stream::Me, 3, "до сбоя")).unwrap();
    w.push(fin(Stream::System, 40, "ещё одна реплика")).unwrap();
    // simulate a crash: no finish, no materialize; plus a torn last line
    drop(w);
    let jp = d.path().join(format!(".lowk/state/live/{id}.jsonl"));
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(&jp)
        .unwrap()
        .write_all(b"{\"t\":\"entry\",\"e\":{\"ty")
        .unwrap();
    // the on-disk .md is stale (written at begin)
    assert!(!v.read_call(&id).unwrap().md.contains("до сбоя"));

    let v2 = Vault::open(d.path()).unwrap();
    let rec = v2.recoverable_calls().unwrap();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0].id, id);
    assert_eq!(rec[0].entries, 2);
    let meta = v2.recover_call(&id).unwrap();
    let md = v2.read_call(&id).unwrap().md;
    assert!(md.contains("**Я:** до сбоя") && md.contains("**Система:** ещё одна реплика"));
    assert!(meta.ended.is_some());
    assert!(v2.recoverable_calls().unwrap().is_empty());
    assert!(!jp.exists());
}

#[test]
fn live_call_blocks_edits_and_merges_external_changes() {
    let (d, v) = open();
    let mut w = v
        .begin_call_with_clock(new_call(FolderRef::Unsorted), clock())
        .unwrap();
    let id = w.id();
    w.push(fin(Stream::Me, 3, "one")).unwrap();
    assert!(v.save_md(&id, "x").is_err());
    assert!(v.delete(&id).is_err());
    assert!(
        v.recoverable_calls().unwrap().is_empty(),
        "live call is not 'recoverable'"
    );

    // external editor adds a note while recording
    let p = d.path().join(w.rel_path());
    let cur = fs::read_to_string(&p).unwrap();
    fs::write(&p, format!("{cur}\n## Notes\n\nmine\n")).unwrap();
    w.push(fin(Stream::Me, 30, "two")).unwrap();
    w.materialize().unwrap();
    assert_eq!(w.take_notice(), Some(WriterNotice::ExternalEditMerged));
    let md = fs::read_to_string(&p).unwrap();
    assert!(md.contains("## Notes\n\nmine") && md.contains("one") && md.contains("two"));
    w.finish().unwrap();
}

#[test]
fn rescan_detects_external_edit_create_move_remove_but_not_own_writes() {
    let (d, v) = open();
    let meta = record(&v, FolderRef::Unsorted);
    assert!(v.rescan().unwrap().is_empty(), "own writes are silent");

    let p = d.path().join(&meta.path);
    let txt = fs::read_to_string(&p).unwrap();
    fs::write(&p, txt.replace("Привет", "Здравствуйте")).unwrap();
    let ch = v.rescan().unwrap();
    assert_eq!(ch.len(), 1);
    assert_eq!((ch[0].kind, ch[0].id), (StoreChangeKind::Updated, meta.id));

    // rename = same id appears at a new path
    let np = p.with_file_name("renamed by user.md");
    fs::rename(&p, &np).unwrap();
    let ch = v.rescan().unwrap();
    assert_eq!((ch[0].kind, ch[0].id), (StoreChangeKind::Moved, meta.id));

    // new file from sync
    let other = txt.replace(&meta.id.to_string(), &Ulid::new().to_string());
    fs::write(d.path().join("Unsorted/Calls/new.md"), other).unwrap();
    assert_eq!(v.rescan().unwrap()[0].kind, StoreChangeKind::Created);

    fs::remove_file(&np).unwrap();
    let ch = v.rescan().unwrap();
    assert_eq!((ch[0].kind, ch[0].id), (StoreChangeKind::Removed, meta.id));
}

#[test]
fn watcher_reports_external_edit() {
    let (d, v) = open();
    let meta = record(&v, FolderRef::Unsorted);
    let w = v.watch(Duration::from_millis(150)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let p = d.path().join(&meta.path);
    let txt = fs::read_to_string(&p).unwrap();
    fs::write(&p, format!("{txt}\n## Notes\n\nhi\n")).unwrap();
    let start = Instant::now();
    let mut got = None;
    while start.elapsed() < Duration::from_secs(10) {
        if let Ok(c) = w.changes().recv_timeout(Duration::from_millis(200)) {
            got = Some(c);
            break;
        }
    }
    let c = got.expect("watcher event");
    assert!(c
        .iter()
        .any(|x| x.id == meta.id && x.kind == StoreChangeKind::Updated));
}

#[test]
fn obsidian_plain_folder_is_adopted() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join("Plain")).unwrap();
    fs::write(d.path().join("Plain/note.md"), "# just a note\n").unwrap();
    let v = Vault::open(d.path()).unwrap();
    let f = v.folders().unwrap();
    assert!(f.iter().any(|m| m.title == "Plain"));
    assert!(d.path().join("Plain/_folder.md").exists());
    assert_eq!(
        fs::read_to_string(d.path().join("Plain/note.md")).unwrap(),
        "# just a note\n"
    );
}

#[test]
fn export_import_is_folder_copy() {
    let (_d, v) = open();
    let meta = record(&v, FolderRef::Unsorted);
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("export");
    v.export_vault(&dest, false).unwrap();
    assert!(dest.join("Unsorted/Calls").is_dir());
    assert!(!dest.join(".lowk").exists());

    // import into an empty vault: copy
    let (_d2, v2) = open();
    let rep = v2.import_vault(&dest).unwrap();
    assert_eq!(rep.files_copied, 1);
    assert_eq!(v2.read_call(&meta.id).unwrap().meta.title, "Weekly sync");
    // again: identical -> skipped
    let rep = v2.import_vault(&dest).unwrap();
    assert_eq!((rep.files_copied, rep.skipped), (0, 1));
    // changed content with same id -> imported under a new id
    let p = dest.join(&meta.path);
    let t = fs::read_to_string(&p).unwrap();
    fs::write(&p, t.replace("Привет", "Другое")).unwrap();
    let rep = v2.import_vault(&dest).unwrap();
    assert_eq!(rep.ids_reassigned, 1);
    assert_eq!(v2.calls(None).unwrap().len(), 2);
    // refuses non-empty destination
    assert!(v.export_vault(&dest, false).is_err());
}

#[test]
fn file_names_are_windows_safe() {
    assert_eq!(sanitize_name("a<b>:c/d\\e|f?g*h"), "a b c d e f g h");
    assert_eq!(sanitize_name("CON"), "CON_");
    assert_eq!(sanitize_name("  .. "), "Untitled");
    assert!(sanitize_name(&"я".repeat(300)).chars().count() <= 120);
}

fn fake_saved_files(d: &std::path::Path, id: &Ulid) {
    for (dir, f) in [("audio", "me.wav"), ("revisions", "r1.md")] {
        let p = d.join(".lowk").join(dir).join(id.to_string());
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join(f), b"x").unwrap();
    }
}

fn saved(d: &std::path::Path, id: &Ulid) -> (bool, bool) {
    let b = |dir: &str| d.join(".lowk").join(dir).join(id.to_string()).exists();
    (b("audio"), b("revisions"))
}

#[test]
fn trash_purge_removes_saved_audio_and_revisions_only_on_permanent_delete() {
    let (d, v) = open();
    let a = record(&v, FolderRef::Unsorted).id;
    let b = record(&v, FolderRef::Unsorted).id;
    let folder = v
        .create_folder(NewFolder {
            title: "Lectures".into(),
            tags: vec![],
            allow_cloud: true,
            instructions: String::new(),
        })
        .unwrap();
    let FolderRef::User(fid) = folder.id.clone() else {
        panic!()
    };
    let c = record(&v, FolderRef::User(fid)).id;
    for id in [&a, &b, &c] {
        fake_saved_files(d.path(), id);
    }
    // moving to the trash keeps everything (restorable)
    v.delete(&a).unwrap();
    v.delete(&b).unwrap();
    v.delete(&fid).unwrap();
    for id in [&a, &b, &c] {
        assert_eq!(saved(d.path(), id), (true, true), "kept in the trash");
    }
    v.restore(&a).unwrap();
    assert_eq!(saved(d.path(), &a), (true, true), "restore keeps them");
    v.delete(&a).unwrap();
    // delete forever: that call only
    v.delete_forever(&b).unwrap();
    assert_eq!(saved(d.path(), &b), (false, false));
    assert_eq!(saved(d.path(), &a), (true, true));
    assert_eq!(
        saved(d.path(), &c),
        (true, true),
        "the folder is still in the trash"
    );
    // delete forever of a folder takes its calls' files along
    v.delete_forever(&fid).unwrap();
    assert_eq!(saved(d.path(), &c), (false, false));
    // empty trash purges the rest
    assert_eq!(saved(d.path(), &a), (true, true));
    v.empty_trash().unwrap();
    assert_eq!(saved(d.path(), &a), (false, false));
}

#[test]
fn empty_trash_purges_calls_inside_a_trashed_folder() {
    let (d, v) = open();
    let folder = v
        .create_folder(NewFolder {
            title: "L".into(),
            tags: vec![],
            allow_cloud: true,
            instructions: String::new(),
        })
        .unwrap();
    let FolderRef::User(fid) = folder.id.clone() else {
        panic!()
    };
    let c = record(&v, FolderRef::User(fid)).id;
    fake_saved_files(d.path(), &c);
    v.delete(&fid).unwrap();
    assert_eq!(saved(d.path(), &c), (true, true));
    v.empty_trash().unwrap();
    assert_eq!(saved(d.path(), &c), (false, false));
}

/// Owner bug: an image Ask whose question was empty lost its `[!lowk-qa]` callout next to the `[!lowk-image]`.
#[test]
fn image_ask_qa_with_empty_question_is_in_the_md_and_reads_back() {
    let (d, v) = open();
    let mut w = v
        .begin_call_with_clock(new_call(FolderRef::Unsorted), clock())
        .unwrap();
    w.push(fin(Stream::System, 3, "Привет")).unwrap();
    let qa = Ulid::new();
    let rel = w.save_asset("000010-ask-x-1.png", &[1, 2, 3]).unwrap();
    w.push(CallEntry::Image {
        at_s: 10,
        rel_path: rel,
        source: format!("ask:{qa}"),
        caption: String::new(),
    })
    .unwrap();
    w.push(CallEntry::Qa {
        id: qa,
        at_s: 10,
        template: "brief".into(),
        model: "m".into(),
        range: "00:00:00-00:00:10".into(),
        question: String::new(),
        answer: "Ответ по скриншоту".into(),
    })
    .unwrap();
    let meta = w.finish_at("2026-10-19T10:31:12+03:00".into()).unwrap();
    let doc = v.read_call_doc(&meta.id).unwrap();
    let qas = doc
        .blocks()
        .iter()
        .filter(|b| matches!(b, lowk_store::call_codec::Block::Qa(_)))
        .count();
    assert_eq!(qas, 1, "{:?}", doc.blocks());
    let md: String = walk(d.path());
    assert!(md.contains("[!lowk-qa]") && md.contains("[!lowk-image]"), "{md}");
}

fn walk(p: &std::path::Path) -> String {
    let mut s = String::new();
    for e in fs::read_dir(p).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            s += &walk(&p);
        } else if p.extension().is_some_and(|x| x == "md") {
            s += &fs::read_to_string(&p).unwrap();
        }
    }
    s
}

/// Owner bug: a call left in the recording state by a crash stayed «идёт запись» forever.
#[test]
fn startup_reconcile_finishes_calls_left_recording() {
    let d = tempfile::tempdir().unwrap();
    let id;
    {
        let v = Vault::open(d.path()).unwrap();
        let mut w = v
            .begin_call_with_clock(new_call(FolderRef::Unsorted), clock())
            .unwrap();
        id = w.id();
        w.push(fin(Stream::Me, 3, "Привет")).unwrap();
        w.push(fin(Stream::System, 9, "Да")).unwrap();
        w.materialize().unwrap();
        std::mem::forget(w); // killed: no End line, journal stays
    }
    let v = Vault::open(d.path()).unwrap();
    assert!(v
        .calls(None)
        .unwrap()
        .iter()
        .any(|c| c.id == id && c.ended.is_none()));
    assert_eq!(v.reconcile_stale_recordings().unwrap(), vec![id]);
    let c = v.calls(None).unwrap().into_iter().find(|c| c.id == id).unwrap();
    assert!(c.ended.is_some() && c.duration_s.is_some_and(|s| s >= 9), "{c:?}");
    assert!(v.recoverable_calls().unwrap().is_empty());
    assert!(v.reconcile_stale_recordings().unwrap().is_empty(), "idempotent");
}

/// Same, when the journal is gone and only the unfinished `.md` is left.
#[test]
fn startup_reconcile_finishes_a_journalless_unfinished_md() {
    let d = tempfile::tempdir().unwrap();
    let id;
    {
        let v = Vault::open(d.path()).unwrap();
        let mut w = v
            .begin_call_with_clock(new_call(FolderRef::Unsorted), clock())
            .unwrap();
        id = w.id();
        w.push(fin(Stream::Me, 3, "Привет")).unwrap();
        w.materialize().unwrap();
        std::mem::forget(w);
        let live = d.path().join(".lowk/state/live");
        for e in fs::read_dir(live).unwrap().flatten() {
            fs::remove_file(e.path()).unwrap();
        }
    }
    let v = Vault::open(d.path()).unwrap();
    assert_eq!(v.reconcile_stale_recordings().unwrap(), vec![id]);
    let c = v.calls(None).unwrap().into_iter().find(|c| c.id == id).unwrap();
    assert!(c.ended.is_some());
}

/// Owner bug: the sidebar «Корзина» counted 6 right after a recording started although the trash was empty.
/// The library list comes from one scan and only trashed calls are `deleted`, also while a call is being written.
#[test]
fn library_list_never_marks_active_or_recording_calls_deleted() {
    let (_d, v) = open();
    let m1 = record(&v, FolderRef::Unsorted);
    let mut w = v
        .begin_call_with_clock(new_call(FolderRef::Unsorted), clock())
        .unwrap();
    w.push(fin(Stream::Me, 3, "идёт запись")).unwrap();
    w.materialize().unwrap();
    for _ in 0..5 {
        let all = v.calls_with_trash().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|c| !c.deleted), "{all:?}");
    }
    v.delete(&m1.id).unwrap();
    let all = v.calls_with_trash().unwrap();
    assert_eq!(all.iter().filter(|c| c.deleted).count(), 1);
    assert_eq!(v.calls(None).unwrap().len(), 1);
    drop(w);
}
