//! Connection settings and the provider profile (request quirks per backend).
use lowk_types::{LlmProvider, LlmSettings};
use serde_json::{json, Map, Value};
use std::time::Duration;

/// Reliable second choice (BRIEF: single upstream, stable p95).
pub const FALLBACK_MODEL: &str = lowk_types::DEFAULT_VISION_MODEL;

/// Which backend dialect to speak. One wire format (OpenAI chat completions), a few quirks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    OpenRouter,
    Ollama,
    LmStudio,
    LlamaServer,
    Custom,
}

impl From<LlmProvider> for ProviderKind {
    fn from(p: LlmProvider) -> Self {
        match p {
            LlmProvider::Openrouter => Self::OpenRouter,
            LlmProvider::Ollama => Self::Ollama,
            LlmProvider::Lmstudio => Self::LmStudio,
            LlmProvider::LlamaServer => Self::LlamaServer,
            LlmProvider::Custom => Self::Custom,
        }
    }
}

/// Everything the client needs. The API key is passed in by the caller (the app reads it from
/// the keychain); it is never logged: `Debug` redacts it.
#[derive(Clone)]
pub struct LlmConfig {
    pub kind: ProviderKind,
    /// `https://openrouter.ai/api/v1`, `http://localhost:11434/v1`, ...
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    /// Tried once when the primary fails with a retryable provider error before any token.
    pub fallback_model: Option<String>,
    /// OpenRouter upstream provider to pin (`provider.order` + `allow_fallbacks:false`).
    pub pinned_provider: Option<String>,
    pub reasoning: bool,
    /// OpenRouter `provider.data_collection = "deny"`.
    pub data_collection_deny: bool,
    /// Stable per call: keeps OpenRouter's sticky routing (prompt cache hits).
    pub session_id: Option<String>,
    pub connect_timeout: Duration,
    /// No first content token within this long: the attempt is abandoned (and retried).
    pub first_token_timeout: Duration,
    /// No bytes for this long after the first token: the stream is abandoned.
    pub idle_timeout: Duration,
    /// Hard cap of one HTTP call, a safety net for threads stuck in a read.
    pub total_timeout: Duration,
    /// Retries before the first token (network, 408/429/5xx). Never after the first token.
    pub max_retries: u32,
    pub retry_backoff: Duration,
    /// Transcript chars per map-reduce part of the summary.
    pub summary_part_chars: usize,
    /// Recording seconds per map-reduce part (by the transcript's `[hh:mm:ss]` stamps).
    pub summary_part_secs: u64,
    /// Joined part notes above this many chars are merged level by level before the final summary.
    pub summary_notes_chars: usize,
}

impl std::fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmConfig")
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .field("pinned_provider", &self.pinned_provider)
            .finish_non_exhaustive()
    }
}

impl LlmConfig {
    pub fn new(kind: ProviderKind, base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            kind,
            base_url: base_url.into(),
            api_key: None,
            model: model.into(),
            fallback_model: None,
            pinned_provider: None,
            reasoning: false,
            data_collection_deny: false,
            session_id: None,
            connect_timeout: Duration::from_secs(10),
            first_token_timeout: Duration::from_secs(45),
            idle_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(300),
            max_retries: 2,
            retry_backoff: Duration::from_millis(300),
            summary_part_chars: 60_000,
            summary_part_secs: 1200,
            summary_notes_chars: 30_000,
        }
    }

    pub fn with_key(mut self, key: Option<String>) -> Self {
        self.api_key = key.filter(|k| !k.trim().is_empty());
        self
    }

    /// Live answers (`llm_ask`): `settings.llm.model`, the user's reasoning choice.
    pub fn for_ask(s: &LlmSettings, key: Option<String>, session_id: Option<String>) -> Self {
        let mut c = Self::new(s.provider.into(), s.base_url.clone(), s.model.clone()).with_key(key);
        c.reasoning = s.reasoning;
        c.data_collection_deny = s.data_collection_deny;
        c.session_id = session_id;
        c.pinned_provider = s.pinned_provider.clone();
        c.fallback_model = c.default_fallback();
        c
    }

    /// Summary and tags: the pinned-version summary model, reasoning off, provider pinned.
    pub fn for_summary(s: &LlmSettings, key: Option<String>) -> Self {
        let mut c = Self::new(s.provider.into(), s.base_url.clone(), s.summary_model.clone()).with_key(key);
        c.data_collection_deny = s.data_collection_deny;
        c.pinned_provider = s.pinned_provider.clone();
        c.fallback_model = c.default_fallback();
        c
    }

    fn default_fallback(&self) -> Option<String> {
        (self.kind == ProviderKind::OpenRouter && self.model != FALLBACK_MODEL)
            .then(|| FALLBACK_MODEL.to_string())
    }

    pub(crate) fn needs_key(&self) -> bool {
        self.kind == ProviderKind::OpenRouter
    }

    pub(crate) fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }

    /// Request fields beyond `model` / `messages`, i.e. the provider profile.
    pub(crate) fn profile_fields(&self, model: &str, opts: &ChatOptions) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("model".into(), json!(model));
        m.insert("stream".into(), json!(true));
        m.insert("stream_options".into(), json!({ "include_usage": true }));
        if let Some(t) = opts.max_tokens {
            m.insert("max_tokens".into(), json!(t));
        }
        if let Some(t) = opts.temperature {
            m.insert("temperature".into(), json!(t));
        }
        match self.kind {
            ProviderKind::OpenRouter => {
                m.insert("reasoning".into(), json!({ "enabled": self.reasoning }));
                let mut p = Map::new();
                // The pin names an upstream for the primary model only.
                if model == self.model {
                    if let Some(up) = &self.pinned_provider {
                        p.insert("order".into(), json!([up]));
                        p.insert("allow_fallbacks".into(), json!(false));
                    }
                }
                if self.data_collection_deny {
                    p.insert("data_collection".into(), json!("deny"));
                }
                if !p.is_empty() {
                    m.insert("provider".into(), Value::Object(p));
                }
                if let Some(s) = opts.session_id.as_ref().or(self.session_id.as_ref()) {
                    m.insert("session_id".into(), json!(s));
                }
            }
            ProviderKind::Ollama => {
                m.insert("keep_alive".into(), json!(-1));
            }
            ProviderKind::LlamaServer => {
                m.insert("cache_prompt".into(), json!(true));
            }
            ProviderKind::LmStudio | ProviderKind::Custom => {}
        }
        m
    }
}

/// Per-request knobs.
#[derive(Clone, Debug, Default)]
pub struct ChatOptions {
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub session_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_key() {
        let c = LlmConfig::new(ProviderKind::OpenRouter, "http://x/api/v1", "m")
            .with_key(Some("sk-or-secret".into()));
        assert!(!format!("{c:?}").contains("secret"));
    }

    #[test]
    fn summary_config_uses_pinned_model_and_fallback() {
        let s = LlmSettings::default();
        let c = LlmConfig::for_summary(&s, Some("k".into()));
        assert_eq!(c.model, lowk_types::DEFAULT_LLM_MODEL_PINNED);
        assert_eq!(c.fallback_model.as_deref(), Some(FALLBACK_MODEL));
        assert!(!c.reasoning);
    }
}
