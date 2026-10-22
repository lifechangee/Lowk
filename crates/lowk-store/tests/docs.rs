use lowk_store::doc_codec;
use lowk_store::*;
use lowk_types::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn open() -> (tempfile::TempDir, Vault) {
    let d = tempfile::tempdir().unwrap();
    let v = Vault::open(&d.path().join("vault")).unwrap();
    (d, v)
}

fn folder(v: &Vault, title: &str) -> (Ulid, FolderRef) {
    let m = v
        .create_folder(NewFolder {
            title: title.into(),
            tags: vec![],
            allow_cloud: true,
            instructions: String::new(),
        })
        .unwrap();
    let FolderRef::User(id) = m.id.clone() else {
        panic!()
    };
    (id, m.id)
}

fn src(d: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let p = d.path().join(name);
    fs::write(&p, bytes).unwrap();
    p
}

fn new(f: &FolderRef, p: &Path) -> NewDocument {
    NewDocument {
        folder: f.clone(),
        source_path: p.to_string_lossy().into(),
        title: None,
        pin: false,
        tags: vec![],
        on_duplicate: Default::default(),
    }
}

fn converted(md: &str) -> IngestedDoc {
    IngestedDoc {
        md: md.into(),
        pages: Some(2),
        warnings: vec![],
        converter: "pdf-text".into(),
        assets: vec![],
    }
}

#[test]
fn golden_doc_roundtrip_is_byte_exact() {
    let g = fs::read_to_string(format!(
        "{}/tests/fixtures/doc_ready.md",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let (d, body) = doc_codec::parse(&g).unwrap();
    assert_eq!(d.kind, DocKind::Pdf);
    assert_eq!(d.status, IngestStatus::Ready);
    assert_eq!(d.tags, vec!["contract", "a, b"]);
    assert_eq!(d.warnings[0].code, IngestWarningCode::OcrUsed);
    assert!(d.pinned && d.pages == Some(2));
    assert_eq!(doc_codec::render(&d, &body), g);
}

#[test]
fn import_keeps_original_and_writes_a_queued_placeholder() {
    let (t, v) = open();
    let (fid, f) = folder(&v, "Клиент Альфа");
    let p = src(&t, "spec_v3.pdf", b"%PDF-1.4 fake");
    let doc = v
        .add_doc(&NewDocument {
            title: Some("Договор: поставки?".into()),
            pin: true,
            tags: vec!["x".into()],
            ..new(&f, &p)
        })
        .unwrap();
    assert_eq!(doc.status, IngestStatus::Queued);
    assert_eq!(doc.kind, DocKind::Pdf);
    assert_eq!(doc.file_name, "spec_v3.pdf");
    assert!(doc.pinned && doc.tags == vec!["x"]);
    assert_eq!(doc.size_bytes, 13);
    assert_eq!(doc.source_sha256.len(), 64);
    assert_eq!(doc.folder, f);
    // sanitized name, original untouched next to the md
    assert_eq!(doc.md_path, "Клиент Альфа/Docs/Договор поставки.md");
    assert_eq!(doc.original_path, "Клиент Альфа/Docs/Договор поставки.pdf");
    let root = v.root().to_path_buf();
    assert_eq!(fs::read(root.join(&doc.original_path)).unwrap(), b"%PDF-1.4 fake");
    assert!(p.exists(), "the source file is copied, not moved");
    // the folder card links the document (Obsidian)
    assert_eq!(v.folder_doc(&fid).unwrap().docs(), ["Docs/Договор поставки"]);
    assert_eq!(v.list_docs(&f).unwrap().len(), 1);
    assert_eq!(v.get_doc(&doc.id).unwrap(), doc);
}

#[test]
fn name_collisions_get_suffixes_and_md_originals_are_safe() {
    let (t, v) = open();
    let (_, f) = folder(&v, "P");
    let a = src(
        &t,
        "notes.md",
        b"---\ntype: doc\nid: 01JC8Z3K6Q9X2M4N7R5T0V8WBZ\n---\n# hi\n",
    );
    let d1 = v.add_doc(&new(&f, &a)).unwrap();
    let d2 = v
        .add_doc(&NewDocument {
            on_duplicate: DupAction::Copy,
            ..new(&f, &a)
        })
        .unwrap();
    assert_eq!(d1.original_path, "P/Docs/notes.md.orig");
    assert_eq!(d2.original_path, "P/Docs/notes (2).md.orig");
    assert_ne!(d1.id, d2.id);
    // the stored markdown original (with its own `type: doc` frontmatter) is not scanned as a document
    assert_eq!(v.list_docs(&f).unwrap().len(), 2);
}

#[test]
fn unsupported_and_missing_sources() {
    let (t, v) = open();
    let (_, f) = folder(&v, "P");
    let p = src(&t, "run.exe", b"MZ");
    assert_eq!(
        v.add_doc(&new(&f, &p)).unwrap_err().code,
        ErrorCode::DocUnsupported
    );
    assert_eq!(
        v.add_doc(&new(&f, &t.path().join("nope.pdf"))).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        v.add_doc(&new(&FolderRef::Trash, &src(&t, "a.txt", b"x")))
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
}

#[test]
fn conversion_lifecycle_and_budget() {
    let (t, v) = open();
    let (_, f) = folder(&v, "P");
    let p = src(&t, "a.pdf", b"%PDF");
    let d = v
        .add_doc(&NewDocument {
            pin: true,
            ..new(&f, &p)
        })
        .unwrap();
    let d = v
        .docs()
        .set_status(&d.id, IngestStatus::Converting, None)
        .unwrap();
    assert_eq!(d.status, IngestStatus::Converting);
    // failure keeps its error, success clears it
    let failed = v
        .docs()
        .set_status(
            &d.id,
            IngestStatus::Failed,
            Some(AppError::new(ErrorCode::DocEncrypted)),
        )
        .unwrap();
    assert_eq!(failed.error.unwrap().code, ErrorCode::DocEncrypted);
    assert_eq!(
        v.docs()
            .set_status(&d.id, IngestStatus::Failed, None)
            .unwrap()
            .error
            .unwrap()
            .code,
        ErrorCode::IngestFailed
    );
    let md = "<!-- p:1 -->\nПоставщик обязуется передать товар.\n\n<!-- p:2 -->\nOK\n";
    let saved = v.docs().save_converted(&d.id, &converted(md)).unwrap();
    assert_eq!(saved.status, IngestStatus::Indexing);
    assert!(saved.error.is_none() && saved.tokens > 0);
    assert_eq!((saved.pages, saved.converter.as_str()), (Some(2), "pdf-text"));
    assert_eq!(v.docs().read_md(&d.id).unwrap(), md);
    assert!(fs::read_to_string(v.root().join(&saved.md_path))
        .unwrap()
        .contains("unit: page"));
    // never an empty document
    assert_eq!(
        v.docs()
            .save_converted(&d.id, &converted("  \n"))
            .unwrap_err()
            .code,
        ErrorCode::DocEmpty
    );
    assert_eq!(v.docs().read_md(&d.id).unwrap(), md);
    // user edit
    let e = v.docs().save_md(&d.id, "# Правка\r\nтекст").unwrap();
    assert_eq!(v.docs().read_md(&d.id).unwrap(), "# Правка\nтекст");
    assert_eq!(e.status, IngestStatus::Indexing);
    v.docs().set_status(&d.id, IngestStatus::Ready, None).unwrap();
    let u = v.pinned_usage(&f, 6000).unwrap();
    assert_eq!((u.count, u.tokens, u.over_budget), (1, e.tokens, false));
    assert!(v.pinned_usage(&f, 1).unwrap().over_budget);
    // slide and sheet units
    let s = v
        .docs()
        .save_converted(
            &d.id,
            &IngestedDoc {
                converter: "pptx".into(),
                ..converted("<!-- p:1 -->\nx slide\n")
            },
        )
        .unwrap();
    assert!(fs::read_to_string(v.root().join(&s.md_path))
        .unwrap()
        .contains("unit: slide"));
}

#[test]
fn patch_pin_tags_title_and_move() {
    let (t, v) = open();
    let (_, a) = folder(&v, "A");
    let (bid, b) = folder(&v, "B");
    let p = src(&t, "x.txt", b"hello text");
    let d = v.add_doc(&new(&a, &p)).unwrap();
    v.docs()
        .save_converted(
            &d.id,
            &IngestedDoc {
                converter: "txt".into(),
                pages: None,
                ..converted("hello text\n")
            },
        )
        .unwrap();
    let d = v
        .patch_doc(
            &d.id,
            &DocPatch {
                title: Some("Новое имя".into()),
                tags: Some(vec!["t".into()]),
                pinned: Some(true),
                folder: None,
            },
        )
        .unwrap();
    assert_eq!(
        (d.title.as_str(), d.pinned, d.tags.clone()),
        ("Новое имя", true, vec!["t".to_string()])
    );
    let moved = v
        .patch_doc(
            &d.id,
            &DocPatch {
                folder: Some(b.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(moved.folder, b);
    assert_eq!(moved.md_path, "B/Docs/x.md");
    assert!(v.root().join("B/Docs/x.txt").exists() && !v.root().join("A/Docs/x.txt").exists());
    assert_eq!(v.docs().read_md(&d.id).unwrap(), "hello text\n");
    assert!(v.list_docs(&a).unwrap().is_empty());
    assert_eq!(v.folder_doc(&bid).unwrap().docs(), ["Docs/x"]);
}

#[test]
fn trash_takes_the_original_along_and_restore_brings_it_back() {
    let (t, v) = open();
    let (fid, f) = folder(&v, "P");
    let p = src(&t, "a.docx", b"PK fake");
    let d = v.add_doc(&new(&f, &p)).unwrap();
    v.delete_doc(&d.id).unwrap();
    assert!(!v.root().join(&d.original_path).exists() && !v.root().join(&d.md_path).exists());
    assert!(v.list_docs(&f).unwrap().is_empty());
    let trashed = v.list_docs(&FolderRef::Trash).unwrap();
    assert_eq!(trashed.len(), 1);
    assert!(trashed[0].deleted);
    assert!(v.folder_doc(&fid).unwrap().docs().is_empty());
    // the generic vault delete / restore API handles documents too
    let back = v.restore_doc(&d.id).unwrap();
    assert!(!back.deleted);
    assert_eq!(fs::read(v.root().join(&back.original_path)).unwrap(), b"PK fake");
    assert_eq!(v.folder_doc(&fid).unwrap().docs(), ["Docs/a"]);
    v.delete(&d.id).unwrap();
    assert_eq!(v.trash_items().unwrap()[0].kind, ItemKind::Doc);
    v.restore(&d.id).unwrap();
    assert_eq!(v.list_docs(&f).unwrap().len(), 1);
    // a collision on restore renames both files consistently
    v.delete(&d.id).unwrap();
    let again = v.add_doc(&new(&f, &p)).unwrap();
    assert_eq!(again.md_path, "P/Docs/a.md");
    let restored = v.restore_doc(&d.id).unwrap();
    assert_eq!(restored.md_path, "P/Docs/a (2).md");
    assert_eq!(restored.original_path, "P/Docs/a (2).docx");
    assert!(v.root().join(&restored.original_path).exists());
}

#[test]
fn external_files_become_queued_imports_on_rescan() {
    let (_t, v) = open();
    let (_, f) = folder(&v, "P");
    let docs = v.root().join("P/Docs");
    fs::create_dir_all(&docs).unwrap();
    fs::write(docs.join("dropped.pdf"), b"%PDF-1.4").unwrap();
    fs::write(docs.join("~$lock.docx"), b"x").unwrap();
    fs::write(docs.join("readme.exe"), b"x").unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    let changes = v.rescan().unwrap();
    let list = v.list_docs(&f).unwrap();
    assert_eq!(
        list.len(),
        1,
        "lock files and unsupported types are ignored: {list:?}"
    );
    assert_eq!(list[0].status, IngestStatus::Queued);
    assert_eq!(list[0].kind, DocKind::Pdf);
    assert!(changes
        .iter()
        .any(|c| c.id == list[0].id && c.kind == StoreChangeKind::Created && !c.is_folder));
    // idempotent
    assert!(v.rescan().unwrap().is_empty());
    assert_eq!(v.list_docs(&f).unwrap().len(), 1);
}

#[test]
fn folder_zip_round_trip_with_documents() {
    let (t, v) = open();
    let (fid, f) = folder(&v, "Клиент");
    let p = src(&t, "spec.pdf", b"%PDF-1.4 body");
    let d = v.add_doc(&new(&f, &p)).unwrap();
    v.docs()
        .save_converted(&d.id, &converted("<!-- p:1 -->\nтекст договора\n"))
        .unwrap();
    let zip = t.path().join("out.zip");
    let n = v.export_folder_zip(&fid, &zip).unwrap();
    assert!(n >= 3);
    assert_eq!(
        v.export_folder_zip(&fid, &zip).unwrap_err().code,
        ErrorCode::AlreadyExists
    );

    // into a second, empty vault
    let v2 = Vault::open(&t.path().join("vault2")).unwrap();
    let rep = v2.import_zip(&zip).unwrap();
    assert!(rep.files_copied >= 3);
    let f2 = v2
        .folders()
        .unwrap()
        .into_iter()
        .find(|m| m.id == f)
        .expect("same folder id");
    let docs = v2.list_docs(&f2.id).unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].id, d.id);
    assert_eq!(
        v2.docs().read_md(&d.id).unwrap(),
        "<!-- p:1 -->\nтекст договора\n"
    );
    assert_eq!(
        fs::read(v2.root().join(&docs[0].original_path)).unwrap(),
        b"%PDF-1.4 body"
    );

    // importing twice changes nothing and never overwrites
    let again = v2.import_zip(&zip).unwrap();
    assert_eq!(again.files_copied, 0);
    assert_eq!(v2.list_docs(&f2.id).unwrap().len(), 1);

    // the same zip into the original vault: identical content is skipped
    let same = v.import_zip(&zip).unwrap();
    assert_eq!(same.files_copied, 0);
}

#[test]
fn hostile_and_broken_archives_are_rejected() {
    use std::io::Write;
    let (t, v) = open();
    let bad = src(&t, "bad.zip", b"not a zip at all");
    assert_eq!(v.import_zip(&bad).unwrap_err().code, ErrorCode::ArchiveInvalid);
    let slip = t.path().join("slip.zip");
    {
        let mut z = zip::ZipWriter::new(fs::File::create(&slip).unwrap());
        z.start_file("../evil.md", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
    }
    assert_eq!(v.import_zip(&slip).unwrap_err().code, ErrorCode::ArchiveInvalid);
    assert!(!t.path().join("evil.md").exists() && !t.path().join("vault/evil.md").exists());
    let empty = t.path().join("empty.zip");
    zip::ZipWriter::new(fs::File::create(&empty).unwrap())
        .finish()
        .unwrap();
    assert_eq!(v.import_zip(&empty).unwrap_err().code, ErrorCode::ArchiveInvalid);
}

#[test]
fn whole_vault_zip() {
    let (t, v) = open();
    let (_, f) = folder(&v, "P");
    v.add_doc(&new(&f, &src(&t, "a.txt", b"hello"))).unwrap();
    let zip = t.path().join("all.zip");
    assert!(v.export_vault_zip(&zip, false).unwrap() >= 3);
    let v2 = Vault::open(&t.path().join("v2")).unwrap();
    v2.import_zip(&zip).unwrap();
    assert_eq!(
        v2.list_docs(&FolderRef::User(match &f {
            FolderRef::User(i) => *i,
            _ => unreachable!(),
        }))
        .unwrap()
        .len(),
        1
    );
}

#[test]
fn duplicate_upload_is_detected_by_content_and_offers_open_copy_replace() {
    let (t, v) = open();
    let (_, f) = folder(&v, "P");
    let (_, other) = folder(&v, "Q");
    let a = src(&t, "contract.txt", b"Supplier delivers goods within ten days.");
    let first = v.add_doc(&new(&f, &a)).unwrap();

    // same bytes under another file name: still a duplicate, reported with the existing document
    let renamed = src(&t, "contract-final.txt", b"Supplier delivers goods within ten days.");
    let hits = v.find_duplicates(&[new(&f, &renamed), new(&f, &src(&t, "x.txt", b"different"))]).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].index, 0);
    assert_eq!(hits[0].existing.id, first.id);
    // another folder is not a duplicate
    assert!(v.find_duplicates(&[new(&other, &renamed)]).unwrap().is_empty());

    // default policy refuses and leaves nothing behind
    let e = v.add_doc(&new(&f, &renamed)).unwrap_err();
    assert_eq!(e.code, ErrorCode::DocDuplicate);
    assert_eq!(e.params.get("id"), Some(&first.id.to_string()));
    assert_eq!(v.list_docs(&f).unwrap().len(), 1);
    assert_eq!(fs::read_dir(v.root().join("P/Docs")).unwrap().count(), 2, "original + md only");
    v.add_doc(&new(&other, &renamed)).unwrap();

    // copy: two documents
    let copy = v
        .add_doc(&NewDocument { on_duplicate: DupAction::Copy, ..new(&f, &renamed) })
        .unwrap();
    assert_eq!(v.list_docs(&f).unwrap().len(), 2);

    // replace: the old ones go to the trash, the new one stays
    let rep = v
        .add_doc(&NewDocument { on_duplicate: DupAction::Replace, ..new(&f, &renamed) })
        .unwrap();
    let live: Vec<_> = v.list_docs(&f).unwrap().into_iter().map(|d| d.id).collect();
    assert_eq!(live, vec![rep.id]);
    let trash: Vec<_> = v.list_docs(&FolderRef::Trash).unwrap().into_iter().map(|d| d.id).collect();
    assert!(trash.contains(&first.id) && trash.contains(&copy.id));
    v.restore_doc(&first.id).unwrap();
    assert_eq!(v.list_docs(&f).unwrap().len(), 2);
}
