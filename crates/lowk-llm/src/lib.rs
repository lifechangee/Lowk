//! Lowk LLM layer: OpenAI-compatible SSE client (OpenRouter, Ollama, LM Studio, llama-server),
//! append-only `ChatLedger`, `PromptBuilder` (context pack in, source refs out), summary and
//! tag suggestions (`LlmService`), latency probe. No OS code, no vault access, no keychain:
//! the caller passes the API key in.
mod client;
mod config;
mod ledger;
pub mod mock;
mod probe;
mod prompt;
mod sse;
mod summary;
mod tags;

pub use client::{ChatClient, FakeLlm, OpenAiClient, StreamOutcome};
pub use config::{ChatOptions, LlmConfig, ProviderKind, FALLBACK_MODEL};
pub use ledger::ChatLedger;
pub use probe::{list_providers, probe_fastest_provider, ProbeOptions, ProbeResult, ProbeSample};
pub use prompt::{BuildInput, BuiltTurn, ImagePart, PromptBuilder, SYSTEM_PROMPT};
pub use sse::{SseEvent, SseParser};
pub use summary::{parse_summary, pick_language, split_parts, Lang};
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

impl LlmService for CloudLlm {
    fn summarize(
        &self,
        input: &SummarizeInput,
        on_delta: &mut dyn FnMut(&str),
        should_continue: &dyn Fn() -> bool,
    ) -> Result<Summary> {
        summary::run(&self.client, input, on_delta, should_continue)
    }

    fn suggest_tags(&self, input: &TagInput) -> Result<Vec<TagSuggestion>> {
        tags::run(&self.client, input)
    }
}
