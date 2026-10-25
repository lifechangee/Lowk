//! Headless end-to-end check of the documents pipeline, run by `lowk --selfcheck-m2` (the bundle's own
//! binary, so `just qa-bundle` exercises the bundled sidecar, OCR and index) and by `cargo test`.
//! It builds a throw-away vault in a temp dir, and uses the same `Services` the app uses:
//!
//! ingest PDF + XLSX (+ DOCX, PPTX, CSV, MD; scanned PDF with `ocr`) -> search finds them ->
//! index rebuild from the vault -> semantic search (when a model file is given) -> local-only
//! folders never reach the cloud (LLM, embeddings, retrieval) -> Ask flow with sources against a
//! stub OpenAI-compatible server (and, with `live`, the real OpenRouter) -> summary and tags ->
//! folder export / import round trip.
//!
//! The stub server is `lowk_llm::mock`; nothing here runs unless the flag is passed.
use super::hints::LlmHints;
use super::{Env, Services, StaticKey};
use lowk_index::embedder::models::ModelStore;
use lowk_llm::mock::{MockServer, Reply};
use lowk_session::{HintGenerator, HintRequest};
use lowk_store::{FileSettingsStore, Vault};
use lowk_types::*;
use parking_lot::{Mutex, RwLock};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default, Clone)]
pub struct Options {
    /// `fixtures/ingest`.
    pub fixtures: PathBuf,
    /// A local embedder model file (the pinned EmbeddingGemma GGUF); `None` skips the semantic checks.
    pub model: Option<PathBuf>,
    /// The scanned PDF needs a platform OCR engine (macOS Vision, Windows OCR).
    pub ocr: bool,
    /// The bundle must carry `lowk-embed-worker`; a dev/test run without it skips that check.
    pub require_worker: bool,
    /// One real OpenRouter ask with the key from `OPENROUTER_API_KEY` (tiny prompt).
    pub live_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
    Skip,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub verdict: Verdict,
    pub name: String,
    pub detail: String,
}

#[derive(Default)]
struct Sink(Mutex<Vec<AppEvent>>);
impl EventSink for Sink {
    fn emit(&self, ev: AppEvent) {
        self.0.lock().push(ev);
    }
}

struct Run {
    checks: Vec<Check>,
}
impl Run {
    fn add(&mut self, ok: bool, name: &str, detail: impl Into<String>) {
        let c = Check {
            verdict: if ok { Verdict::Pass } else { Verdict::Fail },
            name: name.into(),
            detail: detail.into(),
        };
        println!(
            "CHECK {} {} :: {}",
            match c.verdict {
                Verdict::Pass => "PASS",
                _ => "FAIL",
            },
            c.name,
            c.detail
        );
        self.checks.push(c);
    }
    fn skip(&mut self, name: &str, why: &str) {
        println!("CHECK SKIP {name} :: {why}");
        self.checks.push(Check {
            verdict: Verdict::Skip,
            name: name.into(),
            detail: why.into(),
        });
    }
}

fn wait<T>(timeout: Duration, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let t0 = Instant::now();
    loop {
        if let Some(v) = f() {
            return Some(v);
        }
        if t0.elapsed() > timeout {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn q(text: &str, mode: SearchMode) -> SearchQuery {
    SearchQuery {
        text: text.into(),
        filters: SearchFilters::default(),
        mode,
        limit: 10,
        offset: 0,
    }
}

fn new_folder(v: &Vault, title: &str, cloud: bool, ins: &str) -> FolderRef {
    v.create_folder(NewFolder {
        title: title.into(),
        tags: vec![],
        allow_cloud: cloud,
        instructions: ins.into(),
    })
    .expect("create folder")
    .id
}

fn import(svc: &Services, folder: &FolderRef, dir: &Path, file: &str, pin: bool) -> Result<Document> {
    let mut v = svc.import_docs(&[NewDocument {
        folder: folder.clone(),
        source_path: dir.join(file).to_string_lossy().into_owned(),
        title: None,
        pin,
        tags: vec![],
        on_duplicate: Default::default(),
    }])?;
    Ok(v.remove(0))
}

fn wait_ready(svc: &Services, id: &DocId, secs: u64) -> Option<Document> {
    wait(Duration::from_secs(secs), || {
        let d = svc.vault.get_doc(id).ok()?;
        match d.status {
            IngestStatus::Ready => Some(d),
            IngestStatus::Failed | IngestStatus::Cancelled => Some(d),
            _ => None,
        }
    })
}

/// Stub of OpenRouter: chat streams a fixed answer, embeddings return a deterministic vector per
/// input; every request is recorded by the server.
fn stub(answer: &'static [&'static str]) -> MockServer {
    MockServer::start(move |req, _| {
        if req.path.ends_with("/embeddings") {
            let n = req.json()["input"].as_array().map_or(1, Vec::len);
            let data: Vec<serde_json::Value> = (0..n)
                .map(|i| {
                    let v: Vec<f32> = (0..512)
                        .map(|k| (((k + i * 7) % 13) as f32 - 6.0) / 7.0)
                        .collect();
                    serde_json::json!({ "index": i, "embedding": v })
                })
                .collect();
            return Reply::json(200, serde_json::json!({ "data": data }).to_string());
        }
        if req.body.contains("<vocabulary>") {
            return Reply::answer(&["{\"tags\":[{\"tag\":\"договор\",\"confidence\":0.9}]}"]);
        }
        if req.body.contains("## Главное") || req.body.contains("key points") || req.body.contains("Summary")
        {
            return Reply::answer(&[
                "## Главное\n- Обсудили договор поставки\n",
                "## Решения\n- Подписать в пятницу\n## Задачи\n- **Аня:** отправить смету\n## Вопросы ко мне\n- **Когда подписание?** → Мой ответ: в пятницу\n",
            ]);
        }
        Reply::answer(answer)
    })
}

fn hint_req(
    svc: &Services,
    folder: &FolderRef,
    text: &str,
    template: &str,
    first: bool,
    cloud: bool,
) -> HintRequest {
    let FolderRef::User(fid) = folder else {
        unreachable!()
    };
    let s = svc.settings();
    let pack = svc
        .index
        .retrieve_pack(&RetrievalQuery {
            text: text.into(),
            scope: RetrievalScope {
                folder: Some(*fid),
                call: None,
                tags: vec![],
                all_folders: false,
            },
            budget_tokens: s.docs.retrieved_budget_tokens,
            exclude: vec![],
            cloud_provider: cloud,
            pinned_budget_tokens: s.docs.pinned_budget_tokens,
            include_folder_context: first,
            include_previous_calls: true,
        })
        .expect("retrieve_pack");
    HintRequest {
        qa_id: Ulid::new(),
        template_id: template.into(),
        question: text.into(),
        typed_text: Some(text.into()),
        delta: vec![TranscriptLine {
            seq: 1,
            stream: Stream::System,
            offset_s: 65,
            text: text.into(),
            is_final: true,
        }],
        pack,
        first_turn: first,
        sent_chunk_ids: vec![],
        folder: folder.clone(),
        call: Some(Ulid::new()),
        cancelled: Arc::new(AtomicBool::new(false)),
        my_answer: None,
        images: vec![],
        vision_template: false,
    }
}

struct Ctx {
    root: PathBuf,
    env: Env,
    sink: Arc<Sink>,
    settings: Arc<FileSettingsStore>,
}

fn make_ctx(root: &Path, key: Option<String>, model_store: ModelStore) -> Ctx {
    let data = root.join("data");
    std::fs::create_dir_all(&data).expect("data dir");
    let settings = Arc::new(FileSettingsStore::open(&data).expect("settings"));
    let sink = Arc::new(Sink::default());
    let env = Env {
        settings: settings.clone() as Arc<dyn SettingsStore>,
        sink: sink.clone(),
        keys: Arc::new(StaticKey(key)),
        models: model_store,
        embed_worker: None,
    };
    Ctx {
        root: root.to_path_buf(),
        env,
        sink,
        settings,
    }
}

fn patch(c: &Ctx, v: serde_json::Value) {
    c.settings.patch(SettingsPatch(v)).expect("settings patch");
}

/// Runs every check; returns them in order. Prints `CHECK ...` lines as it goes.
pub fn run_checks(o: &Options) -> Vec<Check> {
    let mut r = Run { checks: vec![] };
    let tmp = std::env::temp_dir().join(format!("lowk-selfcheck-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("temp dir");
    let models = ModelStore::new(tmp.join("models"));
    let ctx = make_ctx(&tmp, Some("sk-or-selfcheck".into()), models.clone());
    // semantic checks use the supplied model; otherwise the embedder stays "model missing"
    patch(&ctx, serde_json::json!({ "embedder": { "mode": "local" } }));

    // ---- 1. the sidecar ----------------------------------------------------------------
    match ctx.env.worker() {
        Some(w) => {
            let ok = lowk_platform::process::command(&w)
                .arg("--version")
                .output()
                .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
                .unwrap_or(false);
            r.add(ok, "embed worker runs", w.display().to_string());
        }
        None if o.require_worker => r.add(
            false,
            "embed worker runs",
            "lowk-embed-worker not found next to the executable",
        ),
        None => r.skip("embed worker runs", "no worker in this environment"),
    }

    let vault = Vault::open(&tmp.join("vault")).expect("vault");
    let svc = Services::start(ctx.env.clone(), vault.clone()).expect("services");
    let docs = new_folder(
        &vault,
        "Client Acme",
        true,
        "Клиент Acme: отвечай формально, коротко.",
    );
    let private = new_folder(&vault, "Private", false, "Секретный проект: только локально.");

    // ---- 2. ingest ----------------------------------------------------------------------
    let mut files = vec![
        "text.pdf",
        "sample.xlsx",
        "sample.docx",
        "sample.pptx",
        "sample.csv",
        "sample.md",
    ];
    if o.ocr {
        files.push("scanned.pdf");
    }
    let mut ready = std::collections::HashMap::new();
    for f in &files {
        match import(&svc, &docs, &o.fixtures, f, *f == "sample.md") {
            Ok(d) => match wait_ready(&svc, &d.id, 120) {
                Some(d) if d.status == IngestStatus::Ready => {
                    r.add(
                        true,
                        &format!("ingest {f}"),
                        format!("{} pages={:?} converter={}", d.title, d.pages, d.converter),
                    );
                    ready.insert(*f, d);
                }
                Some(d) => r.add(
                    false,
                    &format!("ingest {f}"),
                    format!("{:?} {:?}", d.status, d.error),
                ),
                None => r.add(false, &format!("ingest {f}"), "timed out"),
            },
            Err(e) => r.add(false, &format!("ingest {f}"), e.to_string()),
        }
    }
    if let Some(d) = ready.get("text.pdf") {
        r.add(
            d.pages == Some(3),
            "pdf keeps its page count",
            format!("{:?}", d.pages),
        );
    }
    if let Some(d) = ready.get("sample.xlsx") {
        r.add(
            d.pages == Some(2),
            "xlsx keeps its sheet count",
            format!("{:?}", d.pages),
        );
    }
    {
        let ev = ctx.sink.0.lock();
        let statuses: Vec<IngestStatus> = ev
            .iter()
            .filter_map(|e| match e {
                AppEvent::Ingest(p) => Some(p.status),
                _ => None,
            })
            .collect();
        let has = |s| statuses.contains(&s);
        r.add(
            has(IngestStatus::Queued)
                && has(IngestStatus::Converting)
                && has(IngestStatus::Indexing)
                && has(IngestStatus::Ready),
            "ingest progress events",
            format!("{} events", statuses.len()),
        );
    }

    // ---- 3. search ----------------------------------------------------------------------
    let find = |svc: &Services, text: &str| svc.index.search(&q(text, SearchMode::Keyword)).expect("search");
    let pdf_id = ready.get("text.pdf").map(|d| d.id);
    let hit = find(&svc, "Поставщик обязуется передать товар");
    r.add(
        pdf_id.is_some_and(|id| {
            hit.hits
                .iter()
                .any(|h| h.source.id == id && h.source.kind == RefKind::Doc && h.source.page.is_some())
        }),
        "search finds the PDF (with its page)",
        format!(
            "{:?}",
            hit.hits
                .iter()
                .map(|h| (
                    h.source.title.clone(),
                    h.source.page,
                    h.snippet.chars().take(40).collect::<String>()
                ))
                .collect::<Vec<_>>()
        ),
    );
    let xl_id = ready.get("sample.xlsx").map(|d| d.id);
    let hit = find(&svc, "Яблоки");
    r.add(
        xl_id.is_some_and(|id| hit.hits.iter().any(|h| h.source.id == id)),
        "search finds the XLSX",
        format!("{} hits", hit.hits.len()),
    );
    let hit = find(&svc, "Участники обсудили план запуска");
    r.add(
        ready
            .get("sample.docx")
            .is_some_and(|d| hit.hits.iter().any(|h| h.source.id == d.id)),
        "search finds the DOCX",
        format!(
            "{} hits; md={:?}",
            hit.hits.len(),
            ready
                .get("sample.docx")
                .and_then(|d| svc.vault.read_doc_md(&d.id).ok())
                .map(|m| m.chars().take(200).collect::<String>())
        ),
    );
    if o.ocr {
        let hit = find(&svc, "Scanned invoice");
        r.add(
            ready
                .get("scanned.pdf")
                .is_some_and(|d| hit.hits.iter().any(|h| h.source.id == d.id)),
            "search finds the scanned PDF (OCR)",
            format!("{} hits", hit.hits.len()),
        );
    }

    // ---- 4. a call in the vault is searchable, summary + tags ------------------------------
    let call = {
        let started = "2026-10-21T10:00:00+00:00".to_string();
        let mut w = vault
            .begin_call_with_clock(
                NewCall {
                    title: "Созвон с Acme".into(),
                    folder: docs.clone(),
                    started,
                    sources: CallSources {
                        me: None,
                        system: None,
                    },
                    stt_engine: None,
                    languages: vec!["ru".into()],
                    speaker_me: "Я".into(),
                    speaker_system: "Система".into(),
                    clock: None,
                },
                SessionClock {
                    t0_mono: 0,
                    t0_wall_ms: 0,
                },
            )
            .expect("begin call");
        for (i, (s, t)) in [
            (
                Stream::System,
                "Давайте обсудим договор поставки и сроки подписания контракта.",
            ),
            (Stream::Me, "Подписание возможно в пятницу, смету отправит Аня."),
        ]
        .into_iter()
        .enumerate()
        {
            w.push(CallEntry::Final {
                event: SttEvent {
                    stream: s,
                    utt: i as u64,
                    rev: 0,
                    kind: SttKind::Final,
                    text: t.into(),
                    lang: Some("ru".into()),
                    t_start_ns: (i as u64 + 1) * 10_000_000_000,
                    t_audio_end_ns: (i as u64 + 1) * 10_000_000_000 + 4_000_000_000,
                    t_emit_ns: (i as u64 + 1) * 10_000_000_000 + 4_000_000_000,
                },
            })
            .expect("push");
        }
        w.finish().expect("finish call")
    };
    ctx.env.emit(AppEvent::Store(StoreChange {
        kind: StoreChangeKind::Updated,
        id: call.id,
        is_folder: false,
    }));
    svc.on_store_change(&StoreChange {
        kind: StoreChangeKind::Updated,
        id: call.id,
        is_folder: false,
    });
    let found = wait(Duration::from_secs(10), || {
        find(&svc, "подписания контракта")
            .hits
            .iter()
            .any(|h| h.source.id == call.id)
            .then_some(())
    });
    r.add(found.is_some(), "a finished call is indexed incrementally", "");

    // ---- 5. the index rebuilds from the vault ----------------------------------------------
    let before: Vec<String> = find(&svc, "Поставщик обязуется передать товар")
        .hits
        .iter()
        .map(|h| h.chunk_id.clone())
        .collect();
    drop(svc);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", super::index_path(&vault).display()));
    }
    let svc = Services::start(ctx.env.clone(), vault.clone()).expect("restart");
    let after = wait(Duration::from_secs(60), || {
        let a: Vec<String> = find(&svc, "Поставщик обязуется передать товар")
            .hits
            .iter()
            .map(|h| h.chunk_id.clone())
            .collect();
        (!a.is_empty()).then_some(a)
    });
    r.add(
        after.as_ref() == Some(&before) && !before.is_empty(),
        "index rebuilds from the vault (deleted index.db, same chunk ids)",
        format!("{} chunks", before.len()),
    );

    // ---- 6. semantic search (real embedder) -------------------------------------------------
    match &o.model {
        None => r.skip("semantic search (embedder)", "no model file given"),
        Some(src) => {
            let dest = lowk_index::embedder::local::model_path(&models);
            std::fs::create_dir_all(dest.parent().expect("model dir")).expect("model dir");
            std::fs::copy(src, &dest).expect("copy model");
            let t0 = Instant::now();
            svc.apply_embedder();
            let ready_sem = wait(Duration::from_secs(300), || {
                (svc.index.status().semantic == SemanticState::Ready).then_some(())
            });
            r.add(
                ready_sem.is_some(),
                "embedder warm: all chunks embedded",
                format!(
                    "{:?} in {} ms",
                    svc.index.status().semantic,
                    t0.elapsed().as_millis()
                ),
            );
            // no shared stems with the document text: only meaning connects them
            let res = svc
                .index
                .search(&q("условия оплаты по контракту на поставку", SearchMode::Hybrid))
                .expect("hybrid");
            r.add(
                res.mode_used == SearchMode::Hybrid
                    && pdf_id.is_some_and(|id| res.hits.iter().take(3).any(|h| h.source.id == id)),
                "hybrid search finds the PDF by meaning",
                format!(
                    "mode={:?} top={:?}",
                    res.mode_used,
                    res.hits.first().map(|h| h.source.title.clone())
                ),
            );
        }
    }

    // ---- 7. Ask flow against a stub: sources, template, language, key, pin -------------------
    let srv = stub(&["Ответ: ", "по документу ", "оплата 1 250 000 руб."]);
    patch(
        &ctx,
        serde_json::json!({
            "llm": { "base_url": srv.base_url(), "pinned_provider": "StubHost" },
            "templates": [
                { "id": "brief", "names": { "ru": "Кратко", "en": "Brief" }, "instruction": "Ответь кратко." },
                { "id": "complement", "names": { "ru": "Дополни мой ответ", "en": "Complement my answer" }, "instruction": "Дополни.", "needs_my_answer": true },
                { "id": "summary", "names": { "ru": "Резюме", "en": "Summary" }, "instruction": "Резюме." },
                { "id": "pirate", "names": { "ru": "Пират", "en": "Pirate" }, "instruction": "Отвечай как пират, одним предложением." }
            ]
        }),
    );
    let hints = LlmHints::new(ctx.env.clone(), Arc::new(RwLock::new(Some(vault.clone()))));
    let req = hint_req(
        &svc,
        &docs,
        "Какие условия оплаты по договору поставки?",
        "pirate",
        true,
        true,
    );
    let mut deltas: Vec<String> = vec![];
    let mut srcs: Vec<SourceRef> = vec![];
    let order = std::cell::RefCell::new(vec![]);
    let out = hints.generate_ex(
        &req,
        &mut |s, _| {
            order.borrow_mut().push("sources");
            srcs = s;
        },
        &mut |d| {
            order.borrow_mut().push("delta");
            deltas.push(d.to_string());
        },
    );
    match out {
        Ok(o) => {
            r.add(
                o.answer.contains("1 250 000") && deltas.len() > 1,
                "ask streams the answer",
                format!("{} deltas", deltas.len()),
            );
            r.add(
                order.borrow().first() == Some(&"sources"),
                "sources arrive before the first delta",
                format!("{:?}", order.borrow())
                    .chars()
                    .take(60)
                    .collect::<String>(),
            );
            r.add(
                pdf_id.is_some_and(|id| srcs.iter().any(|s| s.id == id && s.kind == RefKind::Doc))
                    && srcs.iter().any(|s| s.kind == RefKind::Instructions),
                "sources name the document and the folder instructions",
                format!("{} sources", srcs.len()),
            );
        }
        Err(e) => r.add(false, "ask streams the answer", e.to_string()),
    }
    if let Some(chat) = srv.chat_requests().first() {
        let j = chat.json();
        let body = chat.body.clone();
        r.add(
            body.contains("Отвечай как пират") && body.contains("Клиент Acme") && body.contains("1 250 000"),
            "prompt carries the custom template, folder instructions and the document chunk",
            "",
        );
        r.add(
            j["model"] == DEFAULT_LLM_MODEL_PINNED
                && j["provider"]["order"][0] == "StubHost"
                && j["provider"]["allow_fallbacks"] == false
                && j["reasoning"]["enabled"] == false,
            "request pins version + provider, reasoning off",
            "",
        );
        r.add(
            j["messages"][0]["content"]
                .as_str()
                .is_some_and(|s| s.contains("language of that question")),
            "system prompt answers in the question's language",
            "",
        );
        r.add(
            chat.header("authorization") == Some("Bearer sk-or-selfcheck"),
            "BYOK key sent as bearer",
            "",
        );
    } else {
        r.add(false, "ask reached the stub", "no chat request");
    }
    // no key -> refused with ApiKeyMissing before anything is sent
    let nokey = make_ctx(&tmp.join("nokey"), None, models.clone());
    patch(
        &nokey,
        serde_json::json!({ "llm": { "base_url": srv.base_url() } }),
    );
    let h2 = LlmHints::new(nokey.env.clone(), Arc::new(RwLock::new(Some(vault.clone()))));
    r.add(
        h2.preflight(&docs).err().map(|e| e.code) == Some(ErrorCode::ApiKeyMissing),
        "no API key: the ask is refused with ApiKeyMissing",
        "",
    );

    // ---- 8. local-only folders never reach the cloud ----------------------------------------
    let secret_doc = import(&svc, &private, &o.fixtures, "sample.txt", true).expect("import private");
    let secret_ready = wait_ready(&svc, &secret_doc.id, 60);
    r.add(
        secret_ready
            .as_ref()
            .is_some_and(|d| d.status == IngestStatus::Ready),
        "private folder document converts locally",
        "",
    );
    let n_chat = srv.chat_requests().len();
    let preflight = hints.preflight(&private);
    r.add(
        preflight.as_ref().err().map(|e| e.code) == Some(ErrorCode::LocalOnlyBlocked),
        "local-only folder + cloud LLM: preflight refuses (LocalOnlyBlocked)",
        format!("{preflight:?}"),
    );
    let req = hint_req(&svc, &private, "расскажи про квазар", "brief", true, false);
    let gen = hints.generate_ex(&req, &mut |_, _| {}, &mut |_| {});
    r.add(
        gen.err().map(|e| e.code) == Some(ErrorCode::LocalOnlyBlocked) && srv.chat_requests().len() == n_chat,
        "local-only ask sends nothing to the cloud",
        format!("{} chat requests", srv.chat_requests().len()),
    );
    let cloud_pack = hint_req(&svc, &private, "квазар", "brief", true, true).pack;
    r.add(
        cloud_pack.relevant.is_empty()
            && cloud_pack.pinned.is_empty()
            && cloud_pack.instructions.is_none()
            && cloud_pack.withheld > 0,
        "retrieval withholds a local-only folder from a cloud provider",
        format!("withheld={}", cloud_pack.withheld),
    );
    // cloud embeddings: chunks of the private folder are never sent
    patch(
        &ctx,
        serde_json::json!({ "embedder": { "mode": "cloud", "cloud_model": "stub/embed" } }),
    );
    svc.apply_embedder(); // the app's settings watcher does this on a change
    let embedded = wait(Duration::from_secs(60), || {
        (svc.index.status().semantic == SemanticState::Ready).then_some(())
    });
    let emb_bodies: String = srv
        .requests()
        .into_iter()
        .filter(|q| q.path.ends_with("/embeddings"))
        .map(|q| q.body)
        .collect();
    r.add(
        embedded.is_some() && emb_bodies.contains("1 250 000") && !emb_bodies.contains("квазар-txt"),
        "cloud embeddings skip local-only folders",
        format!("{} bytes sent", emb_bodies.len()),
    );
    // a configured local LLM serves the local-only folder
    patch(
        &ctx,
        serde_json::json!({ "llm": { "local_base_url": srv.base_url(), "local_model": "gemma-local" } }),
    );
    let n_chat = srv.chat_requests().len();
    let req = hint_req(&svc, &private, "расскажи про квазар", "brief", true, false);
    let mut got = String::new();
    let res = hints.generate_ex(&req, &mut |_, _| {}, &mut |d| got.push_str(d));
    let last = srv.chat_requests().into_iter().nth(n_chat);
    r.add(
        res.is_ok()
            && last.as_ref().is_some_and(|l| {
                l.json()["model"] == "gemma-local"
                    && l.header("authorization").is_none()
                    && l.body.contains("квазар-txt")
            }),
        "local-only folder is served by the configured local LLM (no key sent)",
        format!("{:?}", res.err().map(|e| e.code)),
    );
    patch(
        &ctx,
        serde_json::json!({ "llm": { "local_base_url": "", "local_model": "" }, "embedder": { "mode": "off" } }),
    );

    // ---- 9. summary and tags (stub) ------------------------------------------------------------
    ctx.sink.0.lock().clear();
    match svc.summarize(call.id) {
        Ok(()) => {
            let done = wait(Duration::from_secs(30), || {
                ctx.sink.0.lock().iter().find_map(|e| match e {
                    AppEvent::Summary(SummaryEvent::Done { summary, .. }) => Some(Ok(summary.clone())),
                    AppEvent::Summary(SummaryEvent::Error { error, .. }) => Some(Err(error.clone())),
                    _ => None,
                })
            });
            match done {
                Some(Ok(s)) => {
                    let md = vault.read_call(&call.id).map(|c| c.md).unwrap_or_default();
                    r.add(
                        md.contains("Обсудили договор поставки") && !s.key_points.is_empty(),
                        "summary is written into ## Summary",
                        format!("{} key points", s.key_points.len()),
                    );
                }
                other => r.add(false, "summary is written into ## Summary", format!("{other:?}")),
            }
        }
        Err(e) => r.add(false, "summary starts", e.to_string()),
    }
    match svc.suggest_tags(&call.id) {
        Ok(t) => r.add(
            !t.is_empty(),
            "tag suggestions",
            t.iter().map(|x| x.tag.clone()).collect::<Vec<_>>().join(","),
        ),
        Err(e) => r.add(false, "tag suggestions", e.to_string()),
    }
    let blocked = {
        // a call in the local-only folder: the summary is refused before anything is sent
        let mut w = vault
            .begin_call_with_clock(
                NewCall {
                    title: "Секретный созвон".into(),
                    folder: private.clone(),
                    started: "2026-10-21T11:00:00+00:00".into(),
                    sources: CallSources {
                        me: None,
                        system: None,
                    },
                    stt_engine: None,
                    languages: vec!["ru".into()],
                    speaker_me: "Я".into(),
                    speaker_system: "Система".into(),
                    clock: None,
                },
                SessionClock {
                    t0_mono: 0,
                    t0_wall_ms: 0,
                },
            )
            .expect("begin private call");
        w.push(CallEntry::Final {
            event: SttEvent {
                stream: Stream::System,
                utt: 1,
                rev: 0,
                kind: SttKind::Final,
                text: "Секретные условия сделки.".into(),
                lang: Some("ru".into()),
                t_start_ns: 10_000_000_000,
                t_audio_end_ns: 12_000_000_000,
                t_emit_ns: 12_000_000_000,
            },
        })
        .expect("push");
        w.finish().expect("finish")
    };
    let n_chat = srv.chat_requests().len();
    r.add(
        svc.summarize(blocked.id).err().map(|e| e.code) == Some(ErrorCode::LocalOnlyBlocked)
            && svc.suggest_tags(&blocked.id).err().map(|e| e.code) == Some(ErrorCode::LocalOnlyBlocked)
            && srv.chat_requests().len() == n_chat,
        "local-only call: summary and tags are refused, nothing sent",
        "",
    );

    // ---- 10. export / import round trip ---------------------------------------------------------
    let zip = tmp.join("acme.zip");
    let FolderRef::User(did) = &docs else {
        unreachable!()
    };
    match vault.export_folder_zip(did, &zip) {
        Ok(n) => {
            let v2 = Vault::open(&tmp.join("vault2")).expect("vault2");
            let rep = v2.import_zip(&zip);
            let n_docs = v2
                .folders()
                .map(|f| {
                    f.iter()
                        .map(|f| v2.list_docs(&f.id).map(|d| d.len()).unwrap_or(0))
                        .sum::<usize>()
                })
                .unwrap_or(0);
            r.add(
                rep.is_ok() && n_docs == ready.len(),
                "folder export/import round trip",
                format!(
                    "{n} files, {n_docs} documents back, {:?}",
                    rep.err().map(|e| e.code)
                ),
            );
        }
        Err(e) => r.add(false, "folder export/import round trip", e.to_string()),
    }

    // ---- 11. live OpenRouter (opt-in, tiny prompt) --------------------------------------------------
    match &o.live_key {
        None => r.skip("live ask (OpenRouter)", "no key given"),
        Some(k) => {
            let live = make_ctx(&tmp.join("live"), Some(k.clone()), models.clone());
            let h = LlmHints::new(live.env.clone(), Arc::new(RwLock::new(Some(vault.clone()))));
            let mut req = hint_req(
                &svc,
                &docs,
                "Какие условия оплаты по договору поставки?",
                "brief",
                true,
                true,
            );
            req.delta[0].text = "Какие условия оплаты по договору поставки?".into();
            let t0 = Instant::now();
            let mut first: Option<Duration> = None;
            let mut n = 0;
            let mut srcs = 0;
            let res = h.generate_ex(&req, &mut |s, _| srcs = s.len(), &mut |_| {
                n += 1;
                first.get_or_insert_with(|| t0.elapsed());
            });
            match res {
                Ok(o) => r.add(
                    n > 1 && srcs > 0 && !o.answer.trim().is_empty(),
                    "live ask: streamed answer with sources",
                    format!(
                        "ttft={} ms, {n} deltas, {srcs} sources, model={}",
                        first.map_or(0, |d| d.as_millis()),
                        h.model()
                    ),
                ),
                Err(e) => r.add(false, "live ask: streamed answer with sources", e.to_string()),
            }
        }
    }

    drop(svc);
    let _ = ctx.root;
    let _ = std::fs::remove_dir_all(&tmp);
    r.checks
}

/// `lowk --selfcheck-m2 --fixtures DIR [--model GGUF] [--ocr] [--require-worker] [--live]`.
/// Exit code 0 = no failed check. The live key comes from the environment (`OPENROUTER_API_KEY`).
pub fn main(args: &[String]) -> i32 {
    let arg = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |k: &str| args.iter().any(|a| a == k);
    let Some(fixtures) = arg("--fixtures") else {
        eprintln!("usage: lowk --selfcheck-m2 --fixtures <fixtures/ingest> [--model <gguf>] [--ocr] [--require-worker] [--live]");
        return 2;
    };
    let live_key = has("--live")
        .then(|| std::env::var("OPENROUTER_API_KEY").ok())
        .flatten()
        .filter(|k| !k.is_empty());
    let checks = run_checks(&Options {
        fixtures: PathBuf::from(fixtures),
        model: arg("--model").map(PathBuf::from),
        ocr: has("--ocr"),
        require_worker: has("--require-worker"),
        live_key,
    });
    let failed = checks.iter().filter(|c| c.verdict == Verdict::Fail).count();
    println!("SELFCHECK {} checks, {} failed", checks.len(), failed);
    i32::from(failed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole pipeline offline (no model, no OCR, no live key). The semantic and OCR checks run
    /// from `just qa-bundle` against the bundle.
    #[test]
    fn offline_pipeline_has_no_failed_check() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ingest");
        let checks = run_checks(&Options {
            fixtures,
            ..Options::default()
        });
        let failed: Vec<&Check> = checks.iter().filter(|c| c.verdict == Verdict::Fail).collect();
        assert!(failed.is_empty(), "failed checks: {failed:#?}");
        assert!(checks.iter().filter(|c| c.verdict == Verdict::Pass).count() >= 25);
    }

    /// QA review M2: duplicate upload, a document deleted outside the app (Finder), RU/EN stemming.
    #[test]
    fn duplicate_external_delete_and_stemming() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ingest");
        let tmp = std::env::temp_dir().join(format!("lowk-qa-adv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let ctx = make_ctx(&tmp, Some("sk-or-x".into()), ModelStore::new(tmp.join("models")));
        let vault = Vault::open(&tmp.join("vault")).unwrap();
        let svc = Services::start(ctx.env.clone(), vault.clone()).unwrap();
        let f = new_folder(&vault, "Adv", true, "");
        for n in [
            "sample.docx",
            "sample.pptx",
            "sample.md",
            "sample.csv",
            "sample.xlsx",
            "sample.txt",
        ] {
            let d = import(&svc, &f, &fixtures, n, false).unwrap();
            wait_ready(&svc, &d.id, 60).unwrap();
        }
        let a = import(&svc, &f, &fixtures, "text.pdf", false).unwrap();
        // the same file again is refused by default (content hash), and goes in only as an explicit copy
        let e = import(&svc, &f, &fixtures, "text.pdf", false).unwrap_err();
        assert_eq!(e.code, ErrorCode::DocDuplicate);
        let b = svc
            .import_docs(&[NewDocument {
                folder: f.clone(),
                source_path: fixtures.join("text.pdf").to_string_lossy().into_owned(),
                title: None,
                pin: false,
                tags: vec![],
                on_duplicate: DupAction::Copy,
            }])
            .unwrap()
            .remove(0);
        for d in [&a, &b] {
            assert_eq!(wait_ready(&svc, &d.id, 60).unwrap().status, IngestStatus::Ready);
        }
        let docs = vault.list_docs(&f).unwrap();
        eprintln!(
            "DUP: {} docs after importing the same file twice: {:?}",
            docs.len(),
            docs.iter().map(|d| d.title.clone()).collect::<Vec<_>>()
        );
        let kw = |t: &str| svc.index.search(&q(t, SearchMode::Keyword)).unwrap();
        // RU morphology and RU/EN code-switch
        assert!(
            !kw("поставщик обязуется передать товары").hits.is_empty(),
            "RU stemming"
        );
        assert!(!kw("квазар supply").hits.is_empty(), "code-switch");
        assert!(kw("zzzqqq незнакомое").hits.is_empty(), "no garbage hits");
        // delete outside the app: both files of the doc vanish, then a rescan (the watcher does this)
        let (md, orig) = vault.doc_paths(&a.id).unwrap();
        let _ = std::fs::remove_file(&md);
        let _ = std::fs::remove_file(&orig);
        let (md2, orig2) = vault.doc_paths(&b.id).unwrap();
        let _ = std::fs::remove_file(&md2);
        let _ = std::fs::remove_file(&orig2);
        eprintln!("exists after rm: {} {}", md.exists(), orig.exists());
        let changes = vault.rescan().unwrap();
        eprintln!("CHANGES {changes:?}");
        for c in changes {
            svc.on_store_change(&c);
        }
        let gone = wait(Duration::from_secs(10), || {
            (!kw("Поставщик обязуется передать товар")
                .hits
                .iter()
                .any(|h| h.source.id == a.id || h.source.id == b.id))
            .then_some(())
        });
        eprintln!(
            "HITS {:?}",
            kw("Поставщик обязуется передать товар")
                .hits
                .iter()
                .map(|h| (h.source.id, h.source.kind))
                .collect::<Vec<_>>()
        );
        assert!(gone.is_some(), "index still serves a deleted document");
        assert_eq!(vault.list_docs(&f).unwrap().len(), 6);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Regression: a PDF with an UPPERCASE extension, a Cyrillic name and a space failed with «unknown
    /// error». Kind comes from the extension case-insensitively and the whole path is UTF-8 safe;
    /// every failure reaches the UI as a specific code. `LOWK_PROBE_FILE` runs the same path on a real file.
    #[test]
    fn uppercase_cyrillic_pdf_name_imports() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ingest");
        let tmp = std::env::temp_dir().join(format!("lowk-qa-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let ctx = make_ctx(&tmp, Some("sk-or-x".into()), ModelStore::new(tmp.join("models")));
        let vault = Vault::open(&tmp.join("vault")).unwrap();
        let svc = Services::start(ctx.env.clone(), vault.clone()).unwrap();
        let f = new_folder(&vault, "Names", true, "");
        let src = match std::env::var("LOWK_PROBE_FILE") {
            Ok(p) => std::path::PathBuf::from(p),
            Err(_) => {
                let p = tmp.join("Академ справка.PDF");
                std::fs::copy(fixtures.join("text.pdf"), &p).unwrap();
                p
            }
        };
        let d = svc
            .import_docs(&[NewDocument {
                folder: f.clone(),
                source_path: src.to_string_lossy().into_owned(),
                title: None,
                pin: false,
                tags: vec![],
                on_duplicate: Default::default(),
            }])
            .expect("import")
            .remove(0);
        assert_eq!(d.kind, DocKind::Pdf);
        let done = wait_ready(&svc, &d.id, 120).expect("finished");
        eprintln!(
            "NAME-TEST status={:?} error={:?}",
            done.status,
            done.error.as_ref().map(|e| e.code)
        );
        assert_eq!(done.status, IngestStatus::Ready, "{:?}", done.error);
    }
}
