//! The Ask flow's model side: `HintGenerator` over `lowk-llm` (`PromptBuilder` + `ChatLedger` +
//! the streaming OpenAI-compatible client). The session retrieves (folder instructions, pinned
//! documents, relevant chunks, previous calls); this turns the pack and the transcript delta into
//! the prompt, announces the sources, and streams the answer.
//!
//! Local-only policy lives in `preflight` + `route::resolve`: a local-only folder is served by a
//! local model or refused (`LocalOnlyBlocked`) before retrieval; nothing reaches the cloud.
use super::route::{self, Purpose};
use super::{folder_allows_cloud, Env};
use lowk_capture::prep;
use lowk_llm::{
    compact_ledger, BuildInput, ChatClient, ChatLedger, ChatOptions, CompactNeed, CompactPlan, CompactPolicy,
    ImagePart, OpenAiClient, PromptBuilder,
};
use lowk_session::{HintGenerator, HintOutcome, HintRequest};
use lowk_store::Vault;
use lowk_types::*;
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Assumed context window (tokens) when `llm.ask_context_tokens` is 0.
const WINDOW_CLOUD: usize = 128_000;
const WINDOW_CLAUDE: usize = 200_000;
const WINDOW_LOCAL: usize = 16_384;
/// Room kept for the answer on top of the hard budget.
const ANSWER_RESERVE: usize = 1_024;

/// Compaction budget: soft = 60% of the window (background, after an answer), hard = 85% (inline).
fn policy_for(s: &LlmSettings, route: &route::Route) -> CompactPolicy {
    let window = if s.ask_context_tokens > 0 {
        s.ask_context_tokens as usize
    } else if !route.cloud {
        WINDOW_LOCAL
    } else if route.cfg.model.starts_with("anthropic/") {
        WINDOW_CLAUDE
    } else {
        WINDOW_CLOUD
    };
    let mut p = CompactPolicy::for_window(window);
    p.hard_tokens -= ANSWER_RESERVE.min(window / 10);
    p
}

#[derive(Clone)]
struct CallConv {
    call: Option<CallId>,
    model: String,
    ledger: ChatLedger,
}

/// The cheap summary call: the summary model through the same privacy routing (local-only folders
/// only ever reach the local model). `None` = no summariser available -> plain truncation.
fn summary_client(env: &Env, settings: &Settings, local_only: bool) -> Option<OpenAiClient> {
    let r = route::resolve(
        &settings.llm,
        local_only,
        env.keys.openrouter(),
        &Purpose::Summary,
    )
    .ok()?;
    Some(OpenAiClient::new(r.cfg))
}

pub struct LlmHints {
    env: Env,
    vault: Arc<RwLock<Option<Vault>>>,
    conv: Arc<Mutex<Option<CallConv>>>,
    /// A background compaction is running.
    compacting: Arc<AtomicBool>,
    compaction_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Kept between asks so the TLS connection (keep-alive) is warm: TTFT of the next ask.
    client: Mutex<Option<(String, Arc<OpenAiClient>)>>,
    last_model: Mutex<String>,
}

impl LlmHints {
    pub fn new(env: Env, vault: Arc<RwLock<Option<Vault>>>) -> Self {
        let m = env.settings.get().llm.model;
        Self {
            env,
            vault,
            conv: Arc::new(Mutex::new(None)),
            compacting: Arc::new(AtomicBool::new(false)),
            compaction_thread: Mutex::new(None),
            client: Mutex::new(None),
            last_model: Mutex::new(m),
        }
    }

    fn local_only(&self, folder: &FolderRef) -> bool {
        match self.vault.read().as_ref() {
            Some(v) => !folder_allows_cloud(v, folder),
            // no vault: nothing can be retrieved; a call cannot be recording either
            None => false,
        }
    }

    pub fn route(&self, folder: &FolderRef, call: Option<CallId>) -> Result<route::Route> {
        let s = self.env.settings.get();
        route::resolve(
            &s.llm,
            self.local_only(folder),
            self.env.keys.openrouter(),
            &Purpose::Ask {
                session_id: call.map(|c| c.to_string()),
            },
        )
    }

    fn client_for(&self, cfg: &lowk_llm::LlmConfig) -> Arc<OpenAiClient> {
        // everything that shapes the connection or the request profile, except the per-call session id
        let sig = format!(
            "{:?}|{}|{}|{:?}|{}|{}|{}",
            cfg.kind,
            cfg.base_url,
            cfg.model,
            cfg.pinned_provider,
            cfg.reasoning,
            cfg.data_collection_deny,
            cfg.api_key.as_deref().map_or(0, |k| k.len())
        );
        let mut g = self.client.lock();
        if let Some((s, c)) = g.as_ref() {
            if *s == sig && c.config().api_key == cfg.api_key {
                return c.clone();
            }
        }
        let c = Arc::new(OpenAiClient::new(cfg.clone()));
        *g = Some((sig, c.clone()));
        c
    }
}

impl LlmHints {
    /// After an answer: fold the oldest turns into one summary on a worker thread, so the next Ask is
    /// never delayed. The result is applied only if the conversation still starts with the snapshot;
    /// a failed summary falls back to truncation (also in this step), never to an error.
    fn compact_in_background(&self, policy: CompactPolicy, client: Option<OpenAiClient>) {
        if self.compacting.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(snapshot) = self.conv.lock().clone() else {
            self.compacting.store(false, Ordering::SeqCst);
            return;
        };
        let (conv, flag) = (self.conv.clone(), self.compacting.clone());
        let h = std::thread::spawn(move || {
            if let Some(plan) = CompactPlan::new(&snapshot.ledger, policy.keep_recent_turns) {
                let summary = client
                    .as_ref()
                    .and_then(|c| plan.summarize(&snapshot.ledger, c as &dyn ChatClient, 700).ok());
                let mut g = conv.lock();
                if let Some(cur) = g.as_mut() {
                    if cur.call == snapshot.call
                        && cur.model == snapshot.model
                        && plan.still_applies(&snapshot.ledger, &cur.ledger)
                    {
                        cur.ledger = plan.apply(&cur.ledger, summary.as_deref());
                        tracing::info!(summarized = summary.is_some(), "ask conversation compacted");
                    }
                }
            }
            flag.store(false, Ordering::SeqCst);
        });
        *self.compaction_thread.lock() = Some(h);
    }

    /// Waits for a running background compaction (tests, shutdown).
    pub fn wait_compaction(&self) {
        if let Some(h) = self.compaction_thread.lock().take() {
            let _ = h.join();
        }
    }

    /// Compactions applied to the current conversation (debugging).
    pub fn compactions(&self) -> Vec<lowk_llm::CompactionEvent> {
        self.conv
            .lock()
            .as_ref()
            .map(|c| c.ledger.compactions().to_vec())
            .unwrap_or_default()
    }
}

impl HintGenerator for LlmHints {
    fn generate(&self, _req: &HintRequest, _on_delta: &mut dyn FnMut(&str)) -> Result<String> {
        Err(AppError::new(ErrorCode::Unsupported).with_detail("use generate_ex"))
    }

    fn preflight(&self, folder: &FolderRef) -> Result<bool> {
        self.route(folder, None).map(|r| r.cloud)
    }

    fn model(&self) -> String {
        self.last_model.lock().clone()
    }

    fn warm(&self, folder: &FolderRef) {
        // no key / local-only without a local model: nothing to connect to, nothing to report
        let Ok(route) = self.route(folder, None) else {
            return;
        };
        match self.client_for(&route.cfg).warm() {
            Ok(d) => tracing::info!(ms = d.as_millis() as u64, "llm connection warm"),
            Err(e) => tracing::debug!(code = ?e.code, "llm warm-up failed"),
        }
    }

    fn generate_ex(
        &self,
        req: &HintRequest,
        on_sources: &mut dyn FnMut(Vec<SourceRef>, u32),
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<HintOutcome> {
        let settings = self.env.settings.get();
        let has_images = !req.images.is_empty();
        // Screenshots: local-only folders only reach a local vision model, a text-only cloud model hands
        // the Ask to the vision model. A refusal happens here, before anything is sent; the queue stays.
        let mut route = if has_images {
            route::resolve_vision(
                &settings.llm,
                self.local_only(&req.folder),
                self.env.keys.openrouter(),
                req.call.map(|c| c.to_string()),
            )?
        } else {
            self.route(&req.folder, req.call)?
        };
        let template = settings
            .templates
            .iter()
            .find(|t| t.id == req.template_id)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("template"))?;
        route::apply_template_reasoning(&mut route.cfg, template);
        *self.last_model.lock() = route.cfg.model.clone();

        let builder = PromptBuilder::new(&route.cfg.model);
        // append-only conversation per call: the prefix (system, folder context, earlier turns) is
        // byte-stable so provider prompt caches hit; a new call, model or first turn starts over
        let mut conv = {
            let g = self.conv.lock();
            match g.as_ref() {
                Some(c) if !req.first_turn && c.call == req.call && c.model == route.cfg.model => c.clone(),
                _ => CallConv {
                    call: req.call,
                    model: route.cfg.model.clone(),
                    ledger: builder.new_ledger(),
                },
            }
        };
        // Images belong to the new user turn only; a "too large" refusal (413) retries with smaller ones.
        let edges = [prep::SEND_EDGE, prep::RETRY_EDGES[0], prep::RETRY_EDGES[1]];
        let restarted = std::cell::Cell::new(false);
        let fresh_ctx = std::cell::Cell::new(req.first_turn || conv.ledger.len() <= 1);
        let build = |edge: u32| -> Result<(lowk_llm::BuiltTurn, Vec<SentImage>)> {
            let mut parts = Vec::new();
            let mut sent = Vec::new();
            for (n, im) in req.images.iter().enumerate() {
                let p = prep::prepare(&im.png, edge)?;
                parts.push(ImagePart {
                    caption: format!(
                        "screenshot {}{}",
                        n + 1,
                        if im.primary { " (primary monitor)" } else { "" }
                    ),
                    data_url: p.data_url(),
                });
                sent.push(SentImage {
                    id: im.id,
                    ext: p.ext,
                    bytes: Arc::new(p.bytes),
                    width: p.width,
                    height: p.height,
                });
            }
            let built = builder.build(&BuildInput {
                instruction: &template.instruction,
                typed_text: req.typed_text.as_deref(),
                my_answer: req.my_answer.as_deref(),
                pack: Some(&req.pack),
                include_folder_context: fresh_ctx.get(),
                extra_chunks: &[],
                sent_chunk_ids: if restarted.get() { &[] } else { &req.sent_chunk_ids },
                transcript: &req.delta,
                images: &parts,
            });
            Ok((built, sent))
        };
        let (mut built, mut sent_images) = build(edges[0])?;
        on_sources(built.sources.clone(), built.withheld);

        let client = self.client_for(&route.cfg);
        let opts = ChatOptions {
            max_tokens: Some(1024),
            temperature: None,
            session_id: route.cfg.session_id.clone(),
        };
        let cancelled = req.cancelled.clone();
        let policy = policy_for(&settings.llm, &route);
        let local_only = self.local_only(&req.folder);
        // Hard limit: the request would not fit, so compact inline (summary, else truncation). If even
        // the kept turns do not fit, the conversation starts over (the next turn re-sends the folder context).
        if policy.need(&conv.ledger, &built.message) == CompactNeed::Hard {
            let sc = summary_client(&self.env, &settings, local_only);
            if let Some(l) = compact_ledger(&conv.ledger, &policy, sc.as_ref().map(|c| c as &dyn ChatClient))
            {
                tracing::info!(
                    summarized = l.compactions().last().is_some_and(|e| e.summarized),
                    "ask conversation compacted inline"
                );
                conv.ledger = l;
            }
            if policy.need(&conv.ledger, &built.message) == CompactNeed::Hard && conv.ledger.len() > 2 {
                conv.ledger = builder.new_ledger();
                restarted.set(true);
                fresh_ctx.set(true);
                (built, sent_images) = build(edges[0])?;
            }
        }
        let mut attempt = 0;
        let (out, ledger) = loop {
            let mut ledger = conv.ledger.clone();
            ledger.append(built.message.clone());
            match client.complete(&ledger, &opts, on_delta, &|| !cancelled.load(Ordering::SeqCst)) {
                Err(e) if has_images && e.params.get("status").map(String::as_str) == Some("413") => {
                    attempt += 1;
                    if attempt >= edges.len() {
                        return Err(
                            AppError::new(ErrorCode::ImageTooLarge).with_detail(e.detail.unwrap_or_default())
                        );
                    }
                    (built, sent_images) = build(edges[attempt])?;
                }
                r => break (r?, ledger),
            }
        };
        let mut ledger = ledger;

        // the exact streamed text goes into the ledger, so the history stays byte-stable
        ledger.append_assistant(&out.text);
        let soft = policy.need(&ledger, "") != CompactNeed::None;
        conv.ledger = ledger;
        *self.conv.lock() = Some(conv);
        if soft {
            self.compact_in_background(policy, summary_client(&self.env, &settings, local_only));
        }
        if !out.model.is_empty() {
            *self.last_model.lock() = out.model.clone();
        }
        Ok(HintOutcome {
            answer: out.text,
            sent_chunk_ids: built.chunk_ids,
            truncated: out.truncated,
            usage: out.usage,
            images: sent_images,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m2::StaticKey;
    use lowk_index::embedder::models::ModelStore;
    use lowk_llm::mock::{MockServer, Reply};
    use lowk_store::FileSettingsStore;
    use std::sync::atomic::AtomicBool;

    struct Rig {
        dir: std::path::PathBuf,
        hints: LlmHints,
        settings: Arc<FileSettingsStore>,
        vault: Vault,
    }
    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    struct Quiet;
    impl EventSink for Quiet {
        fn emit(&self, _ev: AppEvent) {}
    }

    fn rig(name: &str, base_url: String) -> Rig {
        let dir = std::env::temp_dir().join(format!("lowk-hints-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let settings = Arc::new(FileSettingsStore::open(&dir.join("data")).unwrap());
        settings
            .patch(SettingsPatch(
                serde_json::json!({ "llm": { "base_url": base_url } }),
            ))
            .unwrap();
        let env = Env {
            settings: settings.clone() as Arc<dyn SettingsStore>,
            sink: Arc::new(Quiet),
            keys: Arc::new(StaticKey(Some("sk-test".into()))),
            models: ModelStore::new(dir.join("models")),
            embed_worker: None,
        };
        let vault = Vault::open(&dir.join("vault")).unwrap();
        let hints = LlmHints::new(env, Arc::new(RwLock::new(Some(vault.clone()))));
        Rig {
            dir,
            hints,
            settings,
            vault,
        }
    }

    fn shot(id: u64) -> QueuedImage {
        let (w, h) = (2000u32, 1000u32);
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[(x % 251) as u8, (y % 241) as u8, ((x * y) % 239) as u8, 255]);
            }
        }
        QueuedImage {
            id,
            index: 0,
            primary: true,
            width: w,
            height: h,
            png: Arc::new(lowk_capture::encode_png(&px, w, h).unwrap()),
        }
    }

    fn request(folder: FolderRef, images: Vec<QueuedImage>) -> HintRequest {
        HintRequest {
            qa_id: Ulid::new(),
            template_id: TEMPLATE_BRIEF.into(),
            question: "q".into(),
            typed_text: Some("Какой ответ на тест?".into()),
            delta: vec![],
            pack: ContextPack::default(),
            first_turn: true,
            sent_chunk_ids: vec![],
            folder,
            call: Some(Ulid::new()),
            cancelled: Arc::new(AtomicBool::new(false)),
            my_answer: None,
            images,
            vision_template: false,
        }
    }

    fn run(r: &Rig, req: &HintRequest) -> Result<HintOutcome> {
        r.hints.generate_ex(req, &mut |_, _| {}, &mut |_| {})
    }

    fn image_parts(j: &serde_json::Value) -> Vec<String> {
        j["messages"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
            .filter(|p| p["type"] == "image_url")
            .map(|p| p["image_url"]["url"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn screenshots_go_out_as_image_url_parts_on_the_vision_model() {
        let srv = MockServer::start(|_, _| Reply::answer(&["42"]));
        let r = rig("img", srv.base_url());
        let out = run(&r, &request(FolderRef::Unsorted, vec![shot(1), shot(2)])).unwrap();
        assert_eq!(out.images.len(), 2);
        assert_eq!(out.images[0].id, 1);
        let reqs = srv.chat_requests();
        assert_eq!(reqs.len(), 1);
        let j = reqs[0].json();
        // the default chat model (DeepSeek) cannot read images: the vision model answers
        assert_eq!(j["model"], lowk_types::DEFAULT_VISION_MODEL);
        let parts = image_parts(&j);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|u| u.starts_with("data:image/")));
        // images sit in the newest user turn, after the text of that turn
        let last = j["messages"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(last["role"], "user");
        assert_eq!(last["content"][0]["type"], "text");
        assert!(last["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Какой ответ на тест?"));
        // the system prefix does not mention images or carry parts
        assert!(j["messages"][0]["content"].is_string() || j["messages"][0]["content"][0]["type"] == "text");
    }

    #[test]
    fn a_text_only_ask_sends_no_images_and_keeps_the_chat_model() {
        let srv = MockServer::start(|_, _| Reply::answer(&["ok"]));
        let r = rig("txt", srv.base_url());
        let out = run(&r, &request(FolderRef::Unsorted, vec![])).unwrap();
        assert!(out.images.is_empty());
        let j = srv.chat_requests()[0].json();
        assert_eq!(j["model"], DEFAULT_LLM_MODEL_PINNED);
        assert!(image_parts(&j).is_empty());
    }

    #[test]
    fn a_vision_capable_chat_model_answers_images_itself() {
        let srv = MockServer::start(|_, _| Reply::answer(&["ok"]));
        let r = rig("own", srv.base_url());
        r.settings
            .patch(SettingsPatch(
                serde_json::json!({ "llm": { "model": "google/gemini-2.5-flash" } }),
            ))
            .unwrap();
        run(&r, &request(FolderRef::Unsorted, vec![shot(1)])).unwrap();
        assert_eq!(srv.chat_requests()[0].json()["model"], "google/gemini-2.5-flash");
    }

    #[test]
    fn local_only_folder_refuses_screenshots_without_touching_the_network() {
        let srv = MockServer::start(|_, _| Reply::answer(&["leak"]));
        let r = rig("local", srv.base_url());
        let f = r
            .vault
            .create_folder(NewFolder {
                title: "secret".into(),
                tags: vec![],
                allow_cloud: false,
                instructions: String::new(),
            })
            .unwrap()
            .id;
        // a text-only local model is configured: text would be fine, images are not
        r.settings
            .patch(SettingsPatch(serde_json::json!({
                "llm": { "local_base_url": "http://127.0.0.1:9/v1", "local_model": "qwen3" }
            })))
            .unwrap();
        let e = run(&r, &request(f, vec![shot(1)])).unwrap_err();
        assert_eq!(e.code, ErrorCode::ImageLocalOnly);
        assert!(srv.requests().is_empty(), "nothing reached the cloud server");
    }

    #[test]
    fn too_large_retries_once_smaller_then_succeeds() {
        let srv = MockServer::scripted(vec![
            Reply::json(413, r#"{"error":{"message":"payload too large"}}"#),
            Reply::answer(&["fine"]),
        ]);
        let r = rig("413", srv.base_url());
        let out = run(&r, &request(FolderRef::Unsorted, vec![shot(1)])).unwrap();
        assert_eq!(out.answer, "fine");
        let reqs = srv.chat_requests();
        assert_eq!(reqs.len(), 2);
        let (a, b) = (
            image_parts(&reqs[0].json())[0].len(),
            image_parts(&reqs[1].json())[0].len(),
        );
        assert!(b < a, "second attempt is smaller ({b} < {a})");
        assert!(out.images[0].width <= 1024);
    }

    #[test]
    fn too_large_after_every_downscale_is_a_clear_error() {
        let srv = MockServer::start(|_, _| Reply::json(413, r#"{"error":{"message":"too big"}}"#));
        let r = rig("413x", srv.base_url());
        let e = run(&r, &request(FolderRef::Unsorted, vec![shot(1)])).unwrap_err();
        assert_eq!(e.code, ErrorCode::ImageTooLarge);
        assert_eq!(srv.chat_requests().len(), 3);
    }

    // --- compaction of the per-call conversation ---

    fn is_summary_call(j: &serde_json::Value) -> bool {
        j["messages"][0]["content"]
            .as_str()
            .is_some_and(|c| c.contains("compress the earlier part"))
    }

    fn asks(srv: &MockServer) -> Vec<serde_json::Value> {
        srv.chat_requests()
            .iter()
            .map(|r| r.json())
            .filter(|j| !is_summary_call(j))
            .collect()
    }

    fn summaries(srv: &MockServer) -> usize {
        srv.chat_requests()
            .iter()
            .filter(|r| is_summary_call(&r.json()))
            .count()
    }

    /// Asks `n` times in one call; the question text is long enough to grow the ledger.
    fn ask_n(r: &Rig, folder: FolderRef, call: Ulid, from: usize, n: usize) {
        for i in from..from + n {
            let mut q = request(folder.clone(), vec![]);
            q.call = Some(call);
            q.first_turn = i == 0;
            q.typed_text = Some(format!("question number {i} {}", "w".repeat(1200)));
            run(r, &q).unwrap();
            r.hints.wait_compaction();
        }
    }

    fn mock_with_summary(fail_summary: bool) -> MockServer {
        MockServer::start(move |req, _| {
            if is_summary_call(&req.json()) {
                if fail_summary {
                    Reply::json(400, r#"{"error":{"message":"nope"}}"#)
                } else {
                    Reply::answer(&["SUMMARY OF THE EARLY CALL"])
                }
            } else {
                Reply::answer(&["an answer"])
            }
        })
    }

    fn set_window(r: &Rig, tokens: u32) {
        r.settings
            .patch(SettingsPatch(
                serde_json::json!({ "llm": { "ask_context_tokens": tokens } }),
            ))
            .unwrap();
    }

    #[test]
    fn below_the_budget_the_prefix_only_grows() {
        let srv = mock_with_summary(false);
        let r = rig("cmp-none", srv.base_url());
        ask_n(&r, FolderRef::Unsorted, Ulid::new(), 0, 4);
        assert_eq!(summaries(&srv), 0);
        let a = asks(&srv);
        for w in a.windows(2) {
            let (p, n) = (
                w[0]["messages"].as_array().unwrap(),
                w[1]["messages"].as_array().unwrap(),
            );
            assert_eq!(p[..], n[..p.len()], "earlier messages byte-identical");
            assert_eq!(n[p.len()]["role"], "assistant");
        }
        assert!(r.hints.compactions().is_empty());
    }

    #[test]
    fn crossing_the_soft_budget_compacts_in_the_background_and_keeps_recent_turns() {
        let srv = mock_with_summary(false);
        let r = rig("cmp-soft", srv.base_url());
        set_window(&r, 5000); // soft 3000, hard 3750 tokens
        let call = Ulid::new();
        ask_n(&r, FolderRef::Unsorted, call, 0, 9);
        assert!(summaries(&srv) >= 1, "the threshold triggered a summary call");
        let ev = r.hints.compactions();
        assert!(
            !ev.is_empty()
                && ev
                    .iter()
                    .all(|e| e.summarized && e.tokens_after < e.tokens_before)
        );
        // the next Ask carries: system + first turn verbatim, the summary, then recent turns verbatim
        ask_n(&r, FolderRef::Unsorted, call, 9, 1);
        let last = asks(&srv).pop().unwrap();
        let m = last["messages"].as_array().unwrap();
        let first = asks(&srv)[0]["messages"].clone();
        assert_eq!(m[0], first[0], "system prompt unchanged");
        assert_eq!(m[1], first[1], "first turn (folder context) unchanged");
        assert!(m[2]["content"]
            .as_str()
            .unwrap()
            .contains("SUMMARY OF THE EARLY CALL"));
        let text = serde_json::to_string(&m[3..]).unwrap();
        assert!(text.contains("question number 8 ") && text.contains("question number 9 "));
        assert!(
            !text.contains("question number 1 "),
            "old turns were folded into the summary"
        );
        // between two compactions the prefix is stable again
        ask_n(&r, FolderRef::Unsorted, call, 10, 1);
        let (a, b) = {
            let v = asks(&srv);
            (
                v[v.len() - 2]["messages"].clone(),
                v[v.len() - 1]["messages"].clone(),
            )
        };
        let (a, b) = (a.as_array().unwrap(), b.as_array().unwrap());
        if b[2]["content"] == a[2]["content"] {
            assert_eq!(a[..], b[..a.len()]);
        }
    }

    #[test]
    fn a_failed_summary_falls_back_to_truncation_and_the_ask_still_works() {
        let srv = mock_with_summary(true);
        let r = rig("cmp-fail", srv.base_url());
        set_window(&r, 5000);
        let call = Ulid::new();
        ask_n(&r, FolderRef::Unsorted, call, 0, 10);
        let ev = r.hints.compactions();
        assert!(
            !ev.is_empty() && ev.iter().all(|e| !e.summarized),
            "truncated, not summarised"
        );
        let last = asks(&srv).pop().unwrap();
        let text = serde_json::to_string(&last["messages"]).unwrap();
        assert!(text.contains("omitted to fit the context window"));
        assert!(text.contains("question number 9 "));
    }

    #[test]
    fn local_only_compaction_never_reaches_the_cloud() {
        let cloud = MockServer::start(|_, _| Reply::answer(&["LEAK"]));
        let local = mock_with_summary(false);
        let r = rig("cmp-local", cloud.base_url());
        let f = r
            .vault
            .create_folder(NewFolder {
                title: "secret".into(),
                tags: vec![],
                allow_cloud: false,
                instructions: String::new(),
            })
            .unwrap()
            .id;
        r.settings
            .patch(SettingsPatch(serde_json::json!({
                "llm": { "local_base_url": local.base_url(), "local_model": "qwen3", "ask_context_tokens": 5000 }
            })))
            .unwrap();
        ask_n(&r, f, Ulid::new(), 0, 9);
        assert!(cloud.requests().is_empty(), "nothing reached the cloud server");
        assert!(summaries(&local) >= 1, "the local model summarised");
        assert!(asks(&local).iter().all(|j| j["model"] == "qwen3"));
        assert!(!r.hints.compactions().is_empty());
    }

    #[test]
    fn a_local_only_folder_without_a_local_model_is_refused_not_compacted_in_the_cloud() {
        let cloud = MockServer::start(|_, _| Reply::answer(&["LEAK"]));
        let r = rig("cmp-none-local", cloud.base_url());
        let f = r
            .vault
            .create_folder(NewFolder {
                title: "secret".into(),
                tags: vec![],
                allow_cloud: false,
                instructions: String::new(),
            })
            .unwrap()
            .id;
        assert_eq!(
            run(&r, &request(f, vec![])).unwrap_err().code,
            ErrorCode::LocalOnlyBlocked
        );
        assert!(cloud.requests().is_empty());
    }
}
