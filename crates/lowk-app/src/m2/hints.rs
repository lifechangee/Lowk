//! The Ask flow's model side: `HintGenerator` over `lowk-llm` (`PromptBuilder` + `ChatLedger` +
//! the streaming OpenAI-compatible client). The session retrieves (folder instructions, pinned
//! documents, relevant chunks, previous calls); this turns the pack and the transcript delta into
//! the prompt, announces the sources, and streams the answer.
//!
//! Local-only policy lives in `preflight` + `route::resolve`: a local-only folder is served by a
//! local model or refused (`LocalOnlyBlocked`) before retrieval; nothing reaches the cloud.
use super::route::{self, Purpose};
use super::{folder_allows_cloud, Env};
use lowk_llm::{BuildInput, ChatLedger, ChatOptions, OpenAiClient, PromptBuilder};
use lowk_session::{HintGenerator, HintOutcome, HintRequest};
use lowk_store::Vault;
use lowk_types::*;
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Past this many bytes the conversation starts over (the next turn re-sends the folder context).
const LEDGER_MAX_BYTES: usize = 400_000;

struct CallConv {
    call: Option<CallId>,
    model: String,
    ledger: ChatLedger,
    bytes: usize,
}

pub struct LlmHints {
    env: Env,
    vault: Arc<RwLock<Option<Vault>>>,
    conv: Mutex<Option<CallConv>>,
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
            conv: Mutex::new(None),
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

    fn generate_ex(
        &self,
        req: &HintRequest,
        on_sources: &mut dyn FnMut(Vec<SourceRef>, u32),
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<HintOutcome> {
        let settings = self.env.settings.get();
        let route = self.route(&req.folder, req.call)?;
        let template = settings
            .templates
            .iter()
            .find(|t| t.id == req.template_id)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("template"))?;
        *self.last_model.lock() = route.cfg.model.clone();

        let builder = PromptBuilder::new(&route.cfg.model);
        // append-only conversation per call: the prefix (system, folder context, earlier turns) is
        // byte-stable so provider prompt caches hit; a new call, model or first turn starts over
        let mut conv = {
            let g = self.conv.lock();
            match g.as_ref() {
                Some(c)
                    if !req.first_turn
                        && c.call == req.call
                        && c.model == route.cfg.model
                        && c.bytes < LEDGER_MAX_BYTES =>
                {
                    CallConv {
                        call: c.call,
                        model: c.model.clone(),
                        ledger: c.ledger.clone(),
                        bytes: c.bytes,
                    }
                }
                _ => CallConv {
                    call: req.call,
                    model: route.cfg.model.clone(),
                    ledger: builder.new_ledger(),
                    bytes: 0,
                },
            }
        };
        let built = builder.build(&BuildInput {
            instruction: &template.instruction,
            typed_text: req.typed_text.as_deref(),
            my_answer: req.my_answer.as_deref(),
            pack: Some(&req.pack),
            include_folder_context: req.first_turn || conv.ledger.len() <= 1,
            extra_chunks: &[],
            sent_chunk_ids: &req.sent_chunk_ids,
            transcript: &req.delta,
            images: &[],
        });
        on_sources(built.sources.clone(), built.withheld);
        let mut ledger = conv.ledger.clone();
        ledger.append(built.message.clone());

        let client = self.client_for(&route.cfg);
        let opts = ChatOptions {
            max_tokens: Some(1024),
            temperature: None,
            session_id: route.cfg.session_id.clone(),
        };
        let cancelled = req.cancelled.clone();
        let out = client.complete(&ledger, &opts, on_delta, &|| !cancelled.load(Ordering::SeqCst))?;

        // the exact streamed text goes into the ledger, so the history stays byte-stable
        ledger.append_assistant(&out.text);
        conv.bytes += built.message.len() + out.text.len();
        conv.ledger = ledger;
        *self.conv.lock() = Some(conv);
        if !out.model.is_empty() {
            *self.last_model.lock() = out.model.clone();
        }
        Ok(HintOutcome {
            answer: out.text,
            sent_chunk_ids: built.chunk_ids,
            truncated: out.truncated,
            usage: out.usage,
        })
    }
}
