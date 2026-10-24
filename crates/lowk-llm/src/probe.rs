//! Latency probe: picks the fastest upstream OpenRouter provider for the pinned model
//! (`llm.pinned_provider`), measured as in the early LLM research (A1.4): time to the first
//! content token of a tiny streamed request with the provider forced and no fallbacks.
use crate::client::OpenAiClient;
use crate::config::{ChatOptions, LlmConfig, ProviderKind};
use crate::ledger::ChatLedger;
use lowk_types::{AppError, ErrorCode, Result};
use serde_json::Value;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct ProbeSample {
    /// Slug usable in `provider.order`.
    pub provider: String,
    /// Best of the runs; `None` = every run failed.
    pub ttft_ms: Option<u32>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProbeResult {
    pub model: String,
    /// Fastest provider (smallest best-of-N TTFT).
    pub best: String,
    pub samples: Vec<ProbeSample>,
}

#[derive(Clone, Debug)]
pub struct ProbeOptions {
    pub max_providers: usize,
    pub runs: usize,
}
impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            max_providers: 6,
            runs: 2,
        }
    }
}

/// Providers that serve `model` according to `/models/{id}/endpoints`.
pub fn list_providers(client: &OpenAiClient, model: &str) -> Result<Vec<String>> {
    let v = client.get_json(&format!("models/{model}/endpoints"))?;
    let eps = v
        .pointer("/data/endpoints")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::new(ErrorCode::LlmBadResponse).with_detail("no endpoints list"))?;
    let mut out: Vec<String> = Vec::new();
    for e in eps {
        let slug = e
            .get("tag")
            .and_then(Value::as_str)
            .or_else(|| e.get("provider_name").and_then(Value::as_str));
        if let Some(s) = slug.filter(|s| !s.is_empty()) {
            if !out.iter().any(|o| o == s) {
                out.push(s.to_string());
            }
        }
    }
    Ok(out)
}

pub fn probe_fastest_provider(
    cfg: &LlmConfig,
    model: &str,
    opts: &ProbeOptions,
    should_continue: &dyn Fn() -> bool,
) -> Result<ProbeResult> {
    if cfg.kind != ProviderKind::OpenRouter {
        return Err(AppError::new(ErrorCode::Unsupported).with_detail("provider pinning is OpenRouter only"));
    }
    if cfg.api_key.is_none() {
        return Err(AppError::new(ErrorCode::ApiKeyMissing));
    }
    let base = OpenAiClient::new(cfg.clone());
    let mut providers = list_providers(&base, model)?;
    providers.truncate(opts.max_providers.max(1));
    if providers.is_empty() {
        return Err(AppError::new(ErrorCode::LlmProvider).with_detail("model has no providers"));
    }

    let mut ledger = ChatLedger::new();
    ledger.append_system("Reply with one word.");
    ledger.append_user("Say: ok");
    let chat = ChatOptions {
        max_tokens: Some(8),
        temperature: Some(0.0),
        session_id: None,
    };

    let mut samples = Vec::new();
    for p in providers {
        if !should_continue() {
            return Err(AppError::new(ErrorCode::LlmCancelled));
        }
        // One client per provider: the pin lives in the profile, no fallback, no retries.
        let mut c = cfg.clone();
        c.model = model.to_string();
        c.pinned_provider = Some(p.clone());
        c.fallback_model = None;
        c.max_retries = 0;
        c.first_token_timeout = Duration::from_secs(20);
        let client = OpenAiClient::new(c);
        let mut best: Option<u32> = None;
        let mut error = None;
        for _ in 0..opts.runs.max(1) {
            let t0 = Instant::now();
            let mut first: Option<Duration> = None;
            let r = client.complete(
                &ledger,
                &chat,
                &mut |_| {
                    first.get_or_insert_with(|| t0.elapsed());
                },
                should_continue,
            );
            match (r, first) {
                (Ok(_), Some(d)) => {
                    let ms = d.as_millis() as u32;
                    best = Some(best.map_or(ms, |b| b.min(ms)));
                }
                (Ok(_), None) => error = Some("no content".to_string()),
                (Err(e), _) if e.code == ErrorCode::LlmCancelled => return Err(e),
                (Err(e), _) => error = Some(e.to_string()),
            }
        }
        samples.push(ProbeSample {
            provider: p,
            ttft_ms: best,
            error: if best.is_some() { None } else { error },
        });
    }
    let best = samples
        .iter()
        .filter_map(|s| s.ttft_ms.map(|t| (t, &s.provider)))
        .min()
        .map(|(_, p)| p.clone())
        .ok_or_else(|| AppError::new(ErrorCode::LlmProvider).with_detail("no provider answered"))?;
    Ok(ProbeResult {
        model: model.to_string(),
        best,
        samples,
    })
}
