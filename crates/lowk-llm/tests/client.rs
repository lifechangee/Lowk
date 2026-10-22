//! Streaming client against the in-crate mock SSE server.
use lowk_llm::mock::{content_event, sse_answer, MockServer, Reply};
use lowk_llm::{ChatClient, ChatLedger, ChatOptions, LlmConfig, OpenAiClient, ProviderKind, FALLBACK_MODEL};
use lowk_types::ErrorCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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

fn run(c: &OpenAiClient, l: &ChatLedger) -> (lowk_types::Result<lowk_llm::StreamOutcome>, Vec<String>) {
    let mut deltas = Vec::new();
    let r = c.stream_ex(
        l,
        &ChatOptions::default(),
        &mut |d| deltas.push(d.to_string()),
        &|| true,
    );
    (r, deltas)
}

#[test]
fn streams_deltas_usage_and_request_shape() {
    let s = MockServer::scripted(vec![Reply::answer(&["Прив", "ет, ", "мир"])]);
    let mut c = cfg(&s);
    c.pinned_provider = Some("together".into());
    c.data_collection_deny = true;
    c.session_id = Some("call-1".into());
    let client = OpenAiClient::new(c);
    let (r, deltas) = run(&client, &ledger());
    let o = r.unwrap();
    assert_eq!(deltas, ["Прив", "ет, ", "мир"]);
    assert_eq!(o.text, "Привет, мир");
    assert!(!o.truncated);
    let u = o.usage.unwrap();
    assert_eq!(
        (u.prompt_tokens, u.completion_tokens, u.cached_tokens),
        (120, 30, 64)
    );

    let reqs = s.chat_requests();
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(r.header("authorization"), Some(&*format!("Bearer {KEY}")));
    assert_eq!(r.header("x-session-id"), Some("call-1"));
    let j = r.json();
    assert_eq!(j["model"], "deepseek/deepseek-v4.1-flash-1017");
    assert_eq!(j["stream"], true);
    assert_eq!(j["reasoning"]["enabled"], false);
    assert_eq!(j["provider"]["order"][0], "together");
    assert_eq!(j["provider"]["allow_fallbacks"], false);
    assert_eq!(j["provider"]["data_collection"], "deny");
    assert_eq!(j["session_id"], "call-1");
    assert_eq!(j["messages"][1]["content"], "Привет");
    // the stored message bytes go on the wire unchanged
    for m in ledger().messages() {
        assert!(r.body.contains(m.as_str()));
    }
}

#[test]
fn local_backend_needs_no_key_and_no_openrouter_fields() {
    let s = MockServer::scripted(vec![Reply::answer(&["ok"])]);
    let c = LlmConfig::new(ProviderKind::Ollama, s.base_url(), "gemma4:e4b");
    let client = OpenAiClient::new(c);
    let (r, _) = run(&client, &ledger());
    assert_eq!(r.unwrap().text, "ok");
    let req = &s.chat_requests()[0];
    assert!(req.header("authorization").is_none());
    let j = req.json();
    assert!(j.get("reasoning").is_none() && j.get("provider").is_none());
    assert_eq!(j["keep_alive"], -1);
}

#[test]
fn tolerates_events_cut_anywhere_and_ignores_reasoning() {
    let mut all = String::new();
    all.push_str("data: {\"choices\":[{\"delta\":{\"reasoning\":\"thinking...\"}}]}\r\n\r\n");
    all.push_str(&content_event("Hello"));
    all.push_str("data: not json at all\n\n");
    all.push_str(&content_event(" world"));
    all.push_str("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n");
    // 7-byte pieces cut JSON and UTF-8 anywhere
    let bytes = all.as_bytes();
    let pieces: Vec<String> = bytes
        .chunks(7)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    let s = MockServer::scripted(vec![Reply::Sse {
        pieces,
        piece_delay: Duration::ZERO,
        hang_after: false,
    }]);
    let (r, deltas) = run(&OpenAiClient::new(cfg(&s)), &ledger());
    assert_eq!(r.unwrap().text, "Hello world");
    assert_eq!(deltas.concat(), "Hello world");
}

#[test]
fn json_body_instead_of_sse_is_accepted() {
    let body = r#"{"model":"m","choices":[{"message":{"role":"assistant","content":"plain"},"finish_reason":"stop"}]}"#;
    let s = MockServer::scripted(vec![Reply::json(200, body)]);
    let (r, deltas) = run(&OpenAiClient::new(cfg(&s)), &ledger());
    assert_eq!(r.unwrap().text, "plain");
    assert_eq!(deltas, ["plain"]);
}

#[test]
fn missing_key_fails_before_any_request() {
    let s = MockServer::scripted(vec![Reply::answer(&["x"])]);
    let mut c = cfg(&s);
    c.api_key = None;
    let (r, _) = run(&OpenAiClient::new(c), &ledger());
    assert_eq!(r.unwrap_err().code, ErrorCode::ApiKeyMissing);
    assert!(s.requests().is_empty());
}

#[test]
fn auth_error_maps_and_is_not_retried_and_never_leaks_key() {
    let body = format!(r#"{{"error":{{"code":401,"message":"bad key {KEY}"}}}}"#);
    let s = MockServer::scripted(vec![Reply::json(401, body)]);
    let (r, _) = run(&OpenAiClient::new(cfg(&s)), &ledger());
    let e = r.unwrap_err();
    assert_eq!(e.code, ErrorCode::ApiKeyMissing);
    assert_eq!(s.chat_requests().len(), 1);
    let shown = format!("{e:?} {e}");
    assert!(!shown.contains("SECRET"), "{shown}");
}

#[test]
fn retries_5xx_and_429_before_first_token() {
    let s = MockServer::scripted(vec![
        Reply::json(503, r#"{"error":{"message":"overloaded"}}"#),
        Reply::Json {
            status: 429,
            body: "{}".into(),
            headers: vec![("Retry-After".into(), "0".into())],
        },
        Reply::answer(&["fine"]),
    ]);
    let (r, _) = run(&OpenAiClient::new(cfg(&s)), &ledger());
    assert_eq!(r.unwrap().text, "fine");
    assert_eq!(s.chat_requests().len(), 3);
}

#[test]
fn exhausted_retries_map_to_llm_provider_with_status() {
    let s = MockServer::scripted(vec![Reply::json(500, r#"{"error":{"message":"boom"}}"#)]);
    let mut c = cfg(&s);
    c.fallback_model = None;
    let (r, _) = run(&OpenAiClient::new(c), &ledger());
    let e = r.unwrap_err();
    assert_eq!(e.code, ErrorCode::LlmProvider);
    assert_eq!(e.params.get("status").map(String::as_str), Some("500"));
    assert_eq!(s.chat_requests().len(), 3, "1 + max_retries");
}

#[test]
fn falls_back_to_second_model_once() {
    let s = MockServer::start(|req, _| {
        if req.json()["model"] == FALLBACK_MODEL {
            Reply::answer(&["from fallback"])
        } else {
            Reply::json(502, "{}")
        }
    });
    let mut c = cfg(&s);
    c.pinned_provider = Some("together".into());
    c.fallback_model = Some(FALLBACK_MODEL.into());
    let (r, _) = run(&OpenAiClient::new(c), &ledger());
    let o = r.unwrap();
    assert_eq!(o.text, "from fallback");
    let last = s.chat_requests().pop().unwrap().json();
    assert_eq!(last["model"], FALLBACK_MODEL);
    assert!(
        last.get("provider").is_none(),
        "upstream pin is for the primary model only"
    );
}

#[test]
fn no_retry_after_first_token() {
    let mut pieces = vec![content_event("partial")];
    pieces.push("data: {\"error\":{\"code\":502,\"message\":\"upstream died\"}}\n\n".into());
    let s = MockServer::scripted(vec![Reply::Sse {
        pieces,
        piece_delay: Duration::ZERO,
        hang_after: false,
    }]);
    let mut c = cfg(&s);
    c.fallback_model = Some(FALLBACK_MODEL.into());
    let (r, deltas) = run(&OpenAiClient::new(c), &ledger());
    assert_eq!(r.unwrap_err().code, ErrorCode::LlmProvider);
    assert_eq!(deltas, ["partial"]);
    assert_eq!(s.chat_requests().len(), 1);
}

#[test]
fn midstream_error_before_content_is_retried() {
    let s = MockServer::scripted(vec![
        Reply::Sse {
            pieces: vec!["data: {\"error\":{\"code\":503,\"message\":\"busy\"}}\n\n".into()],
            piece_delay: Duration::ZERO,
            hang_after: false,
        },
        Reply::answer(&["ok"]),
    ]);
    let (r, _) = run(&OpenAiClient::new(cfg(&s)), &ledger());
    assert_eq!(r.unwrap().text, "ok");
}

#[test]
fn length_finish_is_truncated_and_empty_is_bad_response() {
    let mut p = vec![content_event("cut")];
    p.push("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n".into());
    let s = MockServer::scripted(vec![Reply::Sse {
        pieces: p,
        piece_delay: Duration::ZERO,
        hang_after: false,
    }]);
    let (r, _) = run(&OpenAiClient::new(cfg(&s)), &ledger());
    assert!(r.unwrap().truncated);

    let s = MockServer::scripted(vec![Reply::Sse {
        pieces: sse_answer(&[]),
        piece_delay: Duration::ZERO,
        hang_after: false,
    }]);
    let (r, _) = run(&OpenAiClient::new(cfg(&s)), &ledger());
    assert_eq!(r.unwrap_err().code, ErrorCode::LlmBadResponse);
}

#[test]
fn stream_cut_without_done_is_truncated_not_lost() {
    let s = MockServer::scripted(vec![Reply::Sse {
        pieces: vec![content_event("half an ans")],
        piece_delay: Duration::ZERO,
        hang_after: false,
    }]);
    let (r, _) = run(&OpenAiClient::new(cfg(&s)), &ledger());
    let o = r.unwrap();
    assert_eq!(o.text, "half an ans");
    assert!(o.truncated);
}

#[test]
fn idle_stall_after_first_token_is_a_network_error() {
    let s = MockServer::scripted(vec![Reply::stall_after(&["start"])]);
    let t = Instant::now();
    let (r, deltas) = run(&OpenAiClient::new(cfg(&s)), &ledger());
    assert_eq!(r.unwrap_err().code, ErrorCode::Network);
    assert_eq!(deltas, ["start"]);
    assert!(t.elapsed() < Duration::from_secs(4));
    assert_eq!(s.chat_requests().len(), 1, "no retry after a token");
}

#[test]
fn first_token_timeout_retries_then_fails_as_network() {
    let s = MockServer::scripted(vec![Reply::Hang]);
    let mut c = cfg(&s);
    c.first_token_timeout = Duration::from_millis(300);
    c.max_retries = 1;
    c.fallback_model = None;
    let (r, _) = run(&OpenAiClient::new(c), &ledger());
    assert_eq!(r.unwrap_err().code, ErrorCode::Network);
    assert_eq!(s.chat_requests().len(), 2);
}

#[test]
fn connection_refused_is_network() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut c = LlmConfig::new(ProviderKind::Custom, format!("http://127.0.0.1:{port}/v1"), "m");
    c.max_retries = 0;
    let (r, _) = run(&OpenAiClient::new(c), &ledger());
    assert_eq!(r.unwrap_err().code, ErrorCode::Network);
}

#[test]
fn cancel_mid_stream_returns_quickly() {
    let s = MockServer::scripted(vec![Reply::stall_after(&["one"])]);
    let mut c = cfg(&s);
    c.idle_timeout = Duration::from_secs(30);
    let client = OpenAiClient::new(c);
    let cancel = AtomicBool::new(false);
    let mut got = Vec::new();
    let t = Instant::now();
    let r = client.stream_ex(
        &ledger(),
        &ChatOptions::default(),
        &mut |d| {
            got.push(d.to_string());
            cancel.store(true, Ordering::SeqCst);
        },
        &|| !cancel.load(Ordering::SeqCst),
    );
    assert_eq!(r.unwrap_err().code, ErrorCode::LlmCancelled);
    assert_eq!(got, ["one"]);
    assert!(t.elapsed() < Duration::from_secs(2));
}

#[test]
fn cancel_before_start_sends_nothing() {
    let s = MockServer::scripted(vec![Reply::answer(&["x"])]);
    let client = OpenAiClient::new(cfg(&s));
    let r = client.stream_ex(&ledger(), &ChatOptions::default(), &mut |_| {}, &|| false);
    assert_eq!(r.unwrap_err().code, ErrorCode::LlmCancelled);
    assert!(s.requests().is_empty());
}

#[test]
fn invalid_ledger_message_is_rejected() {
    let s = MockServer::scripted(vec![Reply::answer(&["x"])]);
    let mut l = ChatLedger::new();
    l.append("{not json".into());
    let (r, _) = run(&OpenAiClient::new(cfg(&s)), &l);
    assert_eq!(r.unwrap_err().code, ErrorCode::InvalidArgument);
}

#[test]
fn chat_client_trait_stream_works_and_fake_llm_default_stream_ex() {
    let s = MockServer::scripted(vec![Reply::answer(&["a", "b"])]);
    let client = OpenAiClient::new(cfg(&s));
    let mut d = String::new();
    let full = client.stream(&ledger(), &mut |x| d.push_str(x)).unwrap();
    assert_eq!((full.as_str(), d.as_str()), ("ab", "ab"));

    let fake = lowk_llm::FakeLlm("one two".into());
    let o = fake
        .stream_ex(&ledger(), &ChatOptions::default(), &mut |_| {}, &|| true)
        .unwrap();
    assert_eq!(o.text, "one two");
}

#[test]
fn warm_up_sends_only_an_authenticated_get_without_user_data() {
    let s = MockServer::start(|r, _| {
        if r.path.ends_with("/chat/completions") {
            Reply::answer(&["ok"])
        } else {
            Reply::json(200, r#"{"data":{"endpoints":[]}}"#)
        }
    });
    let client = OpenAiClient::new(cfg(&s));
    client.warm().unwrap();
    let reqs = s.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].method, "GET");
    assert!(reqs[0].path.contains("/models/") && reqs[0].path.ends_with("/endpoints"));
    assert!(reqs[0].body.is_empty(), "no body, so no user data");
    assert_eq!(reqs[0].header("authorization"), Some(&format!("Bearer {KEY}")[..]));
    // the real request after the warm-up works on the same client
    let (r, deltas) = run(&client, &ledger());
    assert_eq!(r.unwrap().text, "ok");
    assert_eq!(deltas, ["ok"]);
    // a dead host is an error to log, never a panic
    let dead = MockServer::scripted(vec![Reply::json(200, "{}")]);
    let url = dead.base_url();
    drop(dead);
    let mut c = cfg(&s);
    c.base_url = url;
    assert!(OpenAiClient::new(c).warm().is_err());
}
