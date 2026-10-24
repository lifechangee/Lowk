//! Lowk LLM layer: OpenAI-compatible SSE client (OpenRouter, Ollama, LM Studio, llama-server),
//! append-only `ChatLedger`, `PromptBuilder` (context pack in, source refs out), summary and
//! tag suggestions (`LlmService`), latency probe. No OS code, no vault access, no keychain:
//! the caller passes the API key in.
mod client;
mod compact;
mod config;
mod ledger;
mod local;
pub mod mock;
mod probe;
mod prompt;
mod sse;
mod summary;
mod tags;

pub use client::{ChatClient, FakeLlm, OpenAiClient, StreamOutcome};
pub use compact::{compact_ledger, estimate_tokens, CompactNeed, CompactPlan, CompactPolicy};
pub use config::{ChatOptions, LlmConfig, ProviderKind, FALLBACK_MODEL};
pub use ledger::{ChatLedger, CompactionEvent};
pub use local::{
    detect_local, parse_ollama_tags, parse_openai_models, LocalTarget, DEFAULT_TARGETS, PROBE_TIMEOUT,
};
pub use probe::{list_providers, probe_fastest_provider, ProbeOptions, ProbeResult, ProbeSample};
pub use prompt::{BuildInput, BuiltTurn, ImagePart, PromptBuilder, SYSTEM_PROMPT};
pub use sse::{SseEvent, SseParser};
pub use summary::{parse_summary, part_range, pick_language, split_parts, split_parts_timed, Lang};
pub use tags::{normalize_tag, parse_tags};

use lowk_types::{LlmService, Result, SummarizeInput, Summary, TagInput, TagSuggestion};

/// `LlmService` over any OpenAI-compatible backend (cloud: OpenRouter with the pinned model).
pub struct CloudLlm {
    client: OpenAiClient,
}

impl CloudLlm {
    pub fn new(cfg: LlmConfig) -> Self {
        Self {
            client: OpenAiClient::new(cfg),
        }
    }

    pub fn client(&self) -> &OpenAiClient {
        &self.client
    }

    /// Cheap check for the Settings "test key" button: a tiny real request.
    pub fn test_key(&self) -> Result<()> {
        let mut l = ChatLedger::new();
        l.append_user("Reply: ok");
        let opts = ChatOptions {
            max_tokens: Some(4),
            ..ChatOptions::default()
        };
        self.client.complete(&l, &opts, &mut |_| {}, &|| true).map(|_| ())
    }
}

impl CloudLlm {
    /// Like `summarize`, plus `on_progress(step, total)` before each request of a long (map-reduce)
    /// summary; `total` is an estimate that may grow when extra merge levels are needed.
    pub fn summarize_with_progress(
        &self,
        input: &SummarizeInput,
        on_delta: &mut dyn FnMut(&str),
        on_progress: &mut dyn FnMut(u32, u32),
        should_continue: &dyn Fn() -> bool,
    ) -> Result<Summary> {
        summary::run(&self.client, input, on_delta, on_progress, should_continue)
    }
}

impl LlmService for CloudLlm {
    fn summarize(
        &self,
        input: &SummarizeInput,
        on_delta: &mut dyn FnMut(&str),
        should_continue: &dyn Fn() -> bool,
    ) -> Result<Summary> {
        summary::run(&self.client, input, on_delta, &mut |_, _| {}, should_continue)
    }

    fn suggest_tags(&self, input: &TagInput) -> Result<Vec<TagSuggestion>> {
        tags::run(&self.client, input)
    }
}
