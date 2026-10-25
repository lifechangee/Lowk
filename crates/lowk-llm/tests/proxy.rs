//! The LLM client honours the system / manual proxy: through an HTTP CONNECT and a SOCKS5 mock proxy
//! (local, no real network), re-read per request; loopback servers are never proxied.
use lowk_llm::mock::{MockServer, Reply};
use lowk_llm::{ChatClient, ChatLedger, ChatOptions, LlmConfig, OpenAiClient, ProviderKind};
use lowk_platform::proxy::{
    self,
    mock::{Kind, MockProxy},
};

fn ask(base: &str) -> lowk_types::Result<String> {
    let c = OpenAiClient::new(
        LlmConfig::new(ProviderKind::OpenRouter, base, "m").with_key(Some("sk-test".into())),
    );
    let mut l = ChatLedger::new();
    l.append_user("hi");
    c.stream_ex(&l, &ChatOptions::default(), &mut |_| {}, &|| true)
        .map(|o| o.text)
}

#[test]
fn llm_requests_go_through_the_proxy() {
    let backend = MockServer::scripted(vec![Reply::answer(&["via ", "proxy"])]);
    for kind in [Kind::HttpConnect, Kind::Socks5] {
        let p = MockProxy::start(kind, std::net::SocketAddr::from(([127, 0, 0, 1], backend.port())));
        assert!(proxy::set_manual(&p.url()));
        // a non-loopback name: only the proxy can reach it
        let text = ask(&format!("http://openrouter.mock.test:{}/api/v1", backend.port())).unwrap();
        assert_eq!(text, "via proxy", "{kind:?}");
        assert_eq!(
            p.seen(),
            vec![format!("openrouter.mock.test:{}", backend.port())],
            "{kind:?}"
        );
        // loopback (a local LLM server) is not proxied even with a proxy configured
        assert_eq!(ask(&backend.base_url()).unwrap(), "via proxy");
        assert_eq!(p.seen().len(), 1, "{kind:?}: loopback went direct");
    }

    // The key check / connection check use the same resolution: typed proxy wins, status and latency come back.
    let p = MockProxy::start(
        Kind::HttpConnect,
        std::net::SocketAddr::from(([127, 0, 0, 1], backend.port())),
    );
    assert!(proxy::set_manual(""));
    let r = lowk_llm::net_check(
        &p.url(),
        &format!("http://openrouter.mock.test:{}/api/v1/models", backend.port()),
    );
    assert!(r.ok, "{r:?}");
    assert_eq!(r.status, Some(200));
    assert_eq!(r.via.as_deref(), Some(p.url().as_str()));
    assert_eq!(p.seen().len(), 1);
    let bad = lowk_llm::net_check("ftp://x:1", "http://x.test/");
    assert!(!bad.ok && bad.error.is_some());
}
