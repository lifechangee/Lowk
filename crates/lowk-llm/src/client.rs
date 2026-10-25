//! OpenAI-compatible streaming chat client (OpenRouter, Ollama, LM Studio, llama-server).
//!
//! Blocking and cancellable: a reader thread pumps the response body into a channel, the
//! calling thread parses SSE, enforces timeouts and polls `should_continue`. Cancel = drop
//! the connection. Retries happen only before the first content token.
use crate::config::{ChatOptions, LlmConfig, ProviderKind};
use crate::ledger::ChatLedger;
use crate::sse::SseParser;
use lowk_types::{AppError, ErrorCode, LlmUsage, Result};
use serde_json::Value;
use std::io::Read;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct StreamOutcome {
    pub text: String,
    /// The model stopped because of the token limit (or the stream was cut after content).
    pub truncated: bool,
    pub usage: Option<LlmUsage>,
    /// Model that actually answered (the fallback when the primary failed).
    pub model: String,
}

pub trait ChatClient: Send + Sync {
    /// Streams a completion; `on_delta` receives text deltas. Cancel by drop.
    fn stream(&self, ledger: &ChatLedger, on_delta: &mut dyn FnMut(&str)) -> Result<String>;

    /// Streaming with options, cancellation and metadata. The default adapts `stream`, so
    /// simple implementations (`FakeLlm`) need not change.
    fn stream_ex(
        &self,
        ledger: &ChatLedger,
        _opts: &ChatOptions,
        on_delta: &mut dyn FnMut(&str),
        should_continue: &dyn Fn() -> bool,
    ) -> Result<StreamOutcome> {
        if !should_continue() {
            return Err(AppError::new(ErrorCode::LlmCancelled));
        }
        let text = self.stream(ledger, on_delta)?;
        Ok(StreamOutcome {
            text,
            ..StreamOutcome::default()
        })
    }
}

/// Answers with a fixed string, streamed in small pieces.
pub struct FakeLlm(pub String);
impl ChatClient for FakeLlm {
    fn stream(&self, _ledger: &ChatLedger, on_delta: &mut dyn FnMut(&str)) -> Result<String> {
        for w in self.0.split_inclusive(' ') {
            on_delta(w);
        }
        Ok(self.0.clone())
    }
}

pub struct OpenAiClient {
    cfg: LlmConfig,
    agent: ureq::Agent,
}

/// Failure of one attempt.
struct Fail {
    err: AppError,
    /// Worth another attempt on the same model.
    retry: bool,
    /// Worth trying the fallback model.
    fallback: bool,
    retry_after: Option<Duration>,
}
impl Fail {
    fn fatal(err: AppError) -> Self {
        Self {
            err,
            retry: false,
            fallback: false,
            retry_after: None,
        }
    }
    fn transient(err: AppError) -> Self {
        Self {
            err,
            retry: true,
            fallback: true,
            retry_after: None,
        }
    }
}

enum Msg {
    Data(Vec<u8>),
    Eof,
    Err(String),
}

fn tls_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls_graviola::default_provider())
}

/// 401 is a bad / missing key; a 403 is a network or region block ("Access denied by security policy")
/// unless its message says the credentials are the problem. Everything else is a provider error.
pub(crate) fn status_error_code(status: u16, message: &str) -> ErrorCode {
    match status {
        401 => ErrorCode::ApiKeyMissing,
        403 => {
            let m = message.to_ascii_lowercase();
            if ["api key", "credential", "unauthorized", "invalid key", "no auth"]
                .iter()
                .any(|w| m.contains(w))
            {
                ErrorCode::ApiKeyMissing
            } else {
                ErrorCode::ProviderBlocked
            }
        }
        _ => ErrorCode::LlmProvider,
    }
}

impl OpenAiClient {
    pub fn new(cfg: LlmConfig) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .unversioned_rustls_crypto_provider(tls_provider())
            .root_certs(ureq::tls::RootCerts::WebPki)
            .build();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .http_status_as_error(false)
            .timeout_connect(Some(cfg.connect_timeout))
            .timeout_recv_response(Some(cfg.first_token_timeout.max(Duration::from_secs(1))))
            .timeout_global(Some(cfg.total_timeout))
            .build()
            .into();
        Self { cfg, agent }
    }

    pub fn config(&self) -> &LlmConfig {
        &self.cfg
    }

    /// One completion with retries and the fallback model.
    pub fn complete(
        &self,
        ledger: &ChatLedger,
        opts: &ChatOptions,
        on_delta: &mut dyn FnMut(&str),
        should_continue: &dyn Fn() -> bool,
    ) -> Result<StreamOutcome> {
        if self.cfg.needs_key() && self.cfg.api_key.is_none() {
            return Err(AppError::new(ErrorCode::ApiKeyMissing));
        }
        if ledger.is_empty() {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_detail("empty ledger"));
        }
        for m in ledger.messages() {
            if serde_json::from_str::<Value>(m).is_err() {
                return Err(
                    AppError::new(ErrorCode::InvalidArgument).with_detail("ledger message is not valid JSON")
                );
            }
        }
        let primary = self.run_model(&self.cfg.model, ledger, opts, on_delta, should_continue);
        match primary {
            Ok(o) => Ok(o),
            Err(f) => {
                if f.fallback {
                    if let Some(fb) = self.cfg.fallback_model.as_deref() {
                        if fb != self.cfg.model {
                            tracing::warn!(model = %self.cfg.model, fallback = %fb, "primary model failed, trying fallback");
                            return self
                                .run_model(fb, ledger, opts, on_delta, should_continue)
                                .map_err(|f2| f2.err);
                        }
                    }
                }
                Err(f.err)
            }
        }
    }

    fn run_model(
        &self,
        model: &str,
        ledger: &ChatLedger,
        opts: &ChatOptions,
        on_delta: &mut dyn FnMut(&str),
        should_continue: &dyn Fn() -> bool,
    ) -> std::result::Result<StreamOutcome, Fail> {
        let body = self.build_body(model, ledger, opts);
        let mut attempt = 0;
        loop {
            if !should_continue() {
                return Err(Fail::fatal(AppError::new(ErrorCode::LlmCancelled)));
            }
            let mut emitted = false;
            match self.attempt(model, &body, opts, on_delta, should_continue, &mut emitted) {
                Ok(o) => return Ok(o),
                Err(f) => {
                    if emitted || !f.retry || attempt >= self.cfg.max_retries {
                        return Err(f);
                    }
                    attempt += 1;
                    let wait = f
                        .retry_after
                        .map(|d| d.min(Duration::from_secs(5)))
                        .unwrap_or_else(|| self.cfg.retry_backoff * (1 << (attempt - 1).min(4)));
                    tracing::warn!(attempt, code = ?f.err.code, "llm attempt failed, retrying");
                    if !sleep_cancellable(wait, should_continue) {
                        return Err(Fail::fatal(AppError::new(ErrorCode::LlmCancelled)));
                    }
                }
            }
        }
    }

    /// The request body: profile fields serialized, `messages` spliced in as stored bytes.
    pub(crate) fn build_body(&self, model: &str, ledger: &ChatLedger, opts: &ChatOptions) -> String {
        let head = Value::Object(self.cfg.profile_fields(model, opts)).to_string();
        debug_assert!(head.ends_with('}'));
        format!(
            "{},\"messages\":{}}}",
            &head[..head.len() - 1],
            ledger.to_json_array()
        )
    }

    fn scrub(&self, s: &str) -> String {
        let mut s = s.to_string();
        if let Some(k) = &self.cfg.api_key {
            if !k.is_empty() {
                s = s.replace(k.as_str(), "<key>");
            }
        }
        if s.chars().count() > 300 {
            s = s.chars().take(300).collect::<String>() + "...";
        }
        s
    }

    fn net_err(&self, e: impl std::fmt::Display) -> AppError {
        AppError::new(ErrorCode::Network).with_detail(self.scrub(&e.to_string()))
    }

    fn request(
        &self,
        method_get: bool,
        url: &str,
        body: Option<&str>,
        opts: &ChatOptions,
    ) -> std::result::Result<ureq::http::Response<ureq::Body>, AppError> {
        let auth = self.cfg.api_key.as_ref().map(|k| format!("Bearer {k}"));
        let session = opts.session_id.as_ref().or(self.cfg.session_id.as_ref());
        let res = if method_get {
            let mut r = self.agent.get(url).header("Accept", "application/json");
            if let Some(a) = &auth {
                r = r.header("Authorization", a);
            }
            r.call()
        } else {
            let mut r = self
                .agent
                .post(url)
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream");
            if let Some(a) = &auth {
                r = r.header("Authorization", a);
            }
            if self.cfg.kind == ProviderKind::OpenRouter {
                r = r.header("X-Title", "Lowk");
                if let Some(s) = session {
                    r = r.header("x-session-id", s);
                }
            }
            r.send(body.unwrap_or(""))
        };
        res.map_err(|e| self.net_err(e))
    }

    /// Opens (and pools) the TLS connection to the provider with one authenticated GET that
    /// carries no user data, so the first real request skips DNS + TCP + TLS (~0.3-1 s cold).
    /// Errors are returned for logging only; a failed warm-up changes nothing.
    pub fn warm(&self) -> Result<Duration> {
        let t0 = Instant::now();
        let path = match self.cfg.kind {
            ProviderKind::OpenRouter => format!("models/{}/endpoints", self.cfg.model),
            _ => "models".to_string(),
        };
        let url = self.cfg.endpoint(&path);
        let mut resp = self.request(true, &url, None, &ChatOptions::default())?;
        // drain the body so the connection returns to the pool
        let _ = resp.body_mut().read_to_vec();
        Ok(t0.elapsed())
    }

    /// GET a JSON document from the API (used by the latency probe).
    pub(crate) fn get_json(&self, path: &str) -> Result<Value> {
        let url = self.cfg.endpoint(path);
        let mut resp = self.request(true, &url, None, &ChatOptions::default())?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().map_err(|e| self.net_err(e))?;
        if status >= 400 {
            return Err(self.map_status(status, &text, None).err);
        }
        serde_json::from_str(&text)
            .map_err(|e| AppError::new(ErrorCode::LlmBadResponse).with_detail(self.scrub(&e.to_string())))
    }

    fn map_status(&self, status: u16, body: &str, retry_after: Option<Duration>) -> Fail {
        let msg = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .or_else(|| v.get("message"))
                    .and_then(|m| m.as_str().map(str::to_string))
            })
            .unwrap_or_else(|| body.to_string());
        let detail = self.scrub(&format!("HTTP {status}: {msg}"));
        let code = status_error_code(status, &msg);
        let err = AppError::new(code)
            .with_param("status", status)
            .with_detail(detail);
        let transient = matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504 | 529);
        Fail {
            err,
            retry: transient,
            fallback: transient || matches!(status, 404 | 400 | 402),
            retry_after,
        }
    }

    fn attempt(
        &self,
        model: &str,
        body: &str,
        opts: &ChatOptions,
        on_delta: &mut dyn FnMut(&str),
        should_continue: &dyn Fn() -> bool,
        emitted: &mut bool,
    ) -> std::result::Result<StreamOutcome, Fail> {
        let started = Instant::now();
        let url = self.cfg.endpoint("chat/completions");
        let mut resp = self
            .request(false, &url, Some(body), opts)
            .map_err(Fail::transient)?;
        let status = resp.status().as_u16();
        if status >= 400 {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_secs);
            let text = resp.body_mut().read_to_string().unwrap_or_default();
            return Err(self.map_status(status, &text, retry_after));
        }
        let is_json = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("application/json"));
        let mut state = StreamState::new(model);

        if is_json {
            // A server that ignored `stream:true`.
            let text = resp
                .body_mut()
                .read_to_string()
                .map_err(|e| Fail::transient(self.net_err(e)))?;
            self.handle_data(&text, &mut state, on_delta, emitted)?;
            return self.finish(state);
        }

        let reader = resp.into_body().into_reader();
        let (tx, rx) = mpsc::sync_channel::<Msg>(64);
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(Msg::Eof);
                        break;
                    }
                    Ok(n) => {
                        if tx.send(Msg::Data(buf[..n].to_vec())).is_err() {
                            break; // receiver dropped: cancelled
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Err(e.to_string()));
                        break;
                    }
                }
            }
        });

        let mut parser = SseParser::new();
        let mut last = Instant::now();
        let mut done = false;
        while !done {
            if !should_continue() {
                return Err(Fail::fatal(AppError::new(ErrorCode::LlmCancelled)));
            }
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Msg::Data(b)) => {
                    last = Instant::now();
                    for ev in parser.push(&b) {
                        if self.handle_data(&ev.data, &mut state, on_delta, emitted)? {
                            done = true;
                            break;
                        }
                    }
                }
                Ok(Msg::Eof) | Err(RecvTimeoutError::Disconnected) => {
                    for ev in parser.finish() {
                        if self.handle_data(&ev.data, &mut state, on_delta, emitted)? {
                            break;
                        }
                    }
                    break;
                }
                Ok(Msg::Err(e)) => {
                    let err = self.net_err(e);
                    return Err(if *emitted {
                        Fail::fatal(err)
                    } else {
                        Fail::transient(err)
                    });
                }
                Err(RecvTimeoutError::Timeout) => {
                    let limit_hit = if *emitted {
                        last.elapsed() > self.cfg.idle_timeout
                    } else {
                        started.elapsed() > self.cfg.first_token_timeout
                    };
                    if limit_hit {
                        let err = AppError::new(ErrorCode::Network).with_detail("stream timed out");
                        return Err(if *emitted {
                            Fail::fatal(err)
                        } else {
                            Fail::transient(err)
                        });
                    }
                }
            }
        }
        // EOF without `[DONE]` and without a finish_reason is a cut stream (`truncated`).
        self.finish(state)
    }

    fn finish(&self, state: StreamState) -> std::result::Result<StreamOutcome, Fail> {
        if state.text.trim().is_empty() && state.finish_reason.as_deref() != Some("length") {
            return Err(Fail {
                err: AppError::new(ErrorCode::LlmBadResponse).with_detail(self.scrub(&format!(
                    "empty answer (finish_reason={}, ignored chunks={})",
                    state.finish_reason.as_deref().unwrap_or("none"),
                    state.bad_chunks
                ))),
                retry: false,
                fallback: false,
                retry_after: None,
            });
        }
        let truncated = state.finish_reason.as_deref() == Some("length")
            || (state.finish_reason.is_none() && !state.saw_done);
        Ok(StreamOutcome {
            text: state.text,
            truncated,
            usage: state.usage,
            model: state.model,
        })
    }

    /// One SSE `data:` payload. Returns `true` at `[DONE]`.
    fn handle_data(
        &self,
        data: &str,
        state: &mut StreamState,
        on_delta: &mut dyn FnMut(&str),
        emitted: &mut bool,
    ) -> std::result::Result<bool, Fail> {
        let data = data.trim();
        if data.is_empty() {
            return Ok(false);
        }
        if data == "[DONE]" {
            state.saw_done = true;
            return Ok(true);
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            state.bad_chunks += 1;
            tracing::warn!("ignoring malformed SSE chunk");
            return Ok(false);
        };
        if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
            let code = e
                .get("code")
                .and_then(|c| c.as_u64().or_else(|| c.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(0) as u16;
            let msg = e
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("provider error");
            let mut f = if code >= 400 {
                self.map_status(
                    code,
                    &format!("{{\"error\":{{\"message\":{}}}}}", Value::from(msg)),
                    None,
                )
            } else {
                Fail::transient(AppError::new(ErrorCode::LlmProvider).with_detail(self.scrub(msg)))
            };
            if *emitted {
                f.retry = false;
                f.fallback = false;
            }
            return Err(f);
        }
        if let Some(m) = v.get("model").and_then(|m| m.as_str()) {
            if !m.is_empty() {
                state.model = m.to_string();
            }
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            let n = |p: &str| u.pointer(p).and_then(Value::as_u64).unwrap_or(0) as u32;
            state.usage = Some(LlmUsage {
                prompt_tokens: n("/prompt_tokens"),
                completion_tokens: n("/completion_tokens"),
                cached_tokens: n("/prompt_tokens_details/cached_tokens"),
            });
        }
        if let Some(ch) = v.pointer("/choices/0") {
            if let Some(fr) = ch.get("finish_reason").and_then(Value::as_str) {
                state.finish_reason = Some(fr.to_string());
            }
            // Only visible content; `reasoning` / `reasoning_content` deltas are dropped.
            let text = ch
                .pointer("/delta/content")
                .or_else(|| ch.pointer("/message/content"))
                .and_then(Value::as_str);
            if let Some(t) = text.filter(|t| !t.is_empty()) {
                state.text.push_str(t);
                *emitted = true;
                on_delta(t);
            }
        }
        Ok(false)
    }
}

struct StreamState {
    text: String,
    finish_reason: Option<String>,
    usage: Option<LlmUsage>,
    model: String,
    saw_done: bool,
    bad_chunks: u32,
}
impl StreamState {
    fn new(model: &str) -> Self {
        Self {
            text: String::new(),
            finish_reason: None,
            usage: None,
            model: model.to_string(),
            saw_done: false,
            bad_chunks: 0,
        }
    }
}

fn sleep_cancellable(d: Duration, should_continue: &dyn Fn() -> bool) -> bool {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if !should_continue() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20).min(d));
    }
    true
}

impl ChatClient for OpenAiClient {
    fn stream(&self, ledger: &ChatLedger, on_delta: &mut dyn FnMut(&str)) -> Result<String> {
        self.complete(ledger, &ChatOptions::default(), on_delta, &|| true)
            .map(|o| o.text)
    }

    fn stream_ex(
        &self,
        ledger: &ChatLedger,
        opts: &ChatOptions,
        on_delta: &mut dyn FnMut(&str),
        should_continue: &dyn Fn() -> bool,
    ) -> Result<StreamOutcome> {
        self.complete(ledger, opts, on_delta, should_continue)
    }
}
