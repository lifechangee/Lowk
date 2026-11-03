//! Timing log of LLM requests. Its own test binary: tracing caches call-site interest process-wide, so
//! a capturing subscriber next to tests that run without one misses events.
use lowk_llm::mock::{MockServer, Reply};
use lowk_llm::{ChatLedger, ChatOptions, LlmConfig, OpenAiClient, ProviderKind};
use lowk_types::ErrorCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
struct LogCapture(Arc<Mutex<Vec<String>>>);

impl tracing::Subscriber for LogCapture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut visitor = Fields(Vec::new());
        event.record(&mut visitor);
        self.0
            .lock()
            .unwrap()
            .push(format!("{} {}", event.metadata().name(), visitor.0.join(" ")));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

struct Fields(Vec<String>);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push(format!("{}={value:?}", field.name()));
    }
}

const KEY: &str = "sk-or-test-SECRET-123";

fn cfg(server: &MockServer) -> LlmConfig {
    let mut c = LlmConfig::new(
        ProviderKind::OpenRouter,
        server.base_url(),
        "deepseek/deepseek-v4.1-flash-1017",
    )
    .with_key(Some(KEY.into()));
    c.retry_backoff = Duration::from_millis(10);
    c.first_token_timeout = Duration::from_secs(5);
    c.idle_timeout = Duration::from_millis(400);
    c
}

fn ledger() -> ChatLedger {
    let mut l = ChatLedger::new();
    l.append_system("sys");
    l.append_user("Привет");
    l
}

#[test]
fn request_timing_fields_are_logged_for_success_fallback_and_cancel() {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let subscriber = LogCapture(lines.clone());
    tracing::subscriber::with_default(subscriber, || {
        // other tests in this binary may have run with no subscriber, caching the call sites as disabled
        tracing::callsite::rebuild_interest_cache();
        let success = MockServer::scripted(vec![Reply::answer(&["ok"])]);
        let client = OpenAiClient::new(cfg(&success));
        client
            .complete_kind("ask", &ledger(), &ChatOptions::default(), &mut |_| {}, &|| true)
            .unwrap();

        let fallback = MockServer::scripted(vec![Reply::json(500, "{}"), Reply::answer(&["fb"])]);
        let mut config = cfg(&fallback);
        config.max_retries = 0;
        config.fallback_model = Some("backup-model".into());
        let client = OpenAiClient::new(config);
        client
            .complete_kind(
                "complement",
                &ledger(),
                &ChatOptions::default(),
                &mut |_| {},
                &|| true,
            )
            .unwrap();

        let cancel = MockServer::scripted(vec![Reply::answer(&["never shown"])]);
        let client = OpenAiClient::new(cfg(&cancel));
        let err = client
            .complete_kind("ask", &ledger(), &ChatOptions::default(), &mut |_| {}, &|| false)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::LlmCancelled);
    });
    let all = lines.lock().unwrap().join("\n");
    assert!(all.contains("llm request finished"));
    assert!(
        all.contains("prompt_chars=")
            && all.contains("connect_ms=")
            && all.contains("ttft_ms=")
            && all.contains("total_ms=")
            && all.contains("output_chars=")
    );
    assert!(all.contains("fallback=true") && all.contains("outcome=\"ok\""));
    assert!(all.contains("LlmCancelled"), "{all}");
}
