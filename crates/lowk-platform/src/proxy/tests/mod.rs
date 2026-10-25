use super::*;
use std::collections::HashMap;

fn u(s: &str) -> ProxyUrl {
    ProxyUrl::parse(s).unwrap()
}

#[test]
fn proxy_url_forms() {
    assert_eq!(u("127.0.0.1:10809").display(), "http://127.0.0.1:10809");
    assert_eq!(u("http://h:3128").scheme, Scheme::Http);
    assert_eq!(u("socks5://127.0.0.1:10808").scheme, Scheme::Socks5);
    assert_eq!(u("socks5h://h").port, 1080);
    assert_eq!(u("socks5://h:1").to_url(), "socks5h://h:1");
    let a = u("http://us%40er:p%3Aw@h:8080");
    assert_eq!(
        (a.user.as_deref(), a.pass.as_deref()),
        (Some("us@er"), Some("p:w"))
    );
    assert_eq!(a.to_url(), "http://us%40er:p%3Aw@h:8080");
    assert_eq!(a.display(), "http://h:8080", "no credentials in the display form");
    assert!(!format!("{a:?}").contains("p%3Aw") && !format!("{a:?}").contains("p:w"));
    assert_eq!(u("http://[::1]:9").host, "::1");
    assert!(ProxyUrl::parse("socks4://h:1").is_none());
    assert!(ProxyUrl::parse("ftp://h:1").is_none());
    assert!(ProxyUrl::parse("http://:80").is_none());
    assert!(ProxyUrl::parse("http://h:0").is_none());
    assert!(ProxyUrl::parse("not a proxy").is_none());
}

#[test]
fn windows_proxy_server_forms() {
    let (h, s) = parse_proxy_server("127.0.0.1:10809");
    assert_eq!(
        (h.unwrap().display(), s.unwrap().display()),
        ("http://127.0.0.1:10809".into(), "http://127.0.0.1:10809".into())
    );
    let (h, s) = parse_proxy_server("http=a:1;https=b:2;ftp=c:3");
    assert_eq!(
        (h.unwrap().display(), s.unwrap().display()),
        ("http://a:1".into(), "http://b:2".into())
    );
    // socks only
    let (h, s) = parse_proxy_server("socks=127.0.0.1:10808");
    assert_eq!(h.unwrap().display(), "socks5://127.0.0.1:10808");
    assert_eq!(s.unwrap().scheme, Scheme::Socks5);
    // http preferred over socks
    let (h, s) = parse_proxy_server("http=127.0.0.1:10809;https=127.0.0.1:10809;socks=127.0.0.1:10808");
    assert_eq!(h.unwrap().scheme, Scheme::Http);
    assert_eq!(s.unwrap().scheme, Scheme::Http);
    let (h, s) = parse_proxy_server("");
    assert!(h.is_none() && s.is_none());
}

#[test]
fn windows_enable_and_override() {
    assert!(parse_windows(false, "127.0.0.1:1", "").is_none(), "ProxyEnable=0");
    assert!(parse_windows(true, "", "").is_none());
    let c = parse_windows(
        true,
        "127.0.0.1:10809",
        "localhost;127.*;10.*;192.168.*;*.corp.example;<local>",
    )
    .unwrap();
    assert_eq!(c.source, Source::Windows);
    assert!(c.for_target("openrouter.ai", true).is_some());
    for h in [
        "localhost",
        "127.0.0.5",
        "10.1.2.3",
        "192.168.0.7",
        "host.corp.example",
        "intranet",
    ] {
        assert!(c.for_target(h, true).is_none(), "{h} bypasses");
    }
    assert!(c.for_target("corp.example.com", true).is_some());
    assert!(c.for_target("192.169.0.1", true).is_some());
}

fn mac(pairs: Vec<(&str, MacVal)>) -> HashMap<String, MacVal> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}
fn n(v: i64) -> MacVal {
    MacVal::Num(v)
}
fn s(v: &str) -> MacVal {
    MacVal::Str(v.into())
}

#[test]
fn macos_dictionary() {
    let d = mac(vec![
        ("HTTPEnable", n(1)),
        ("HTTPProxy", s("127.0.0.1")),
        ("HTTPPort", n(10809)),
        ("HTTPSEnable", n(1)),
        ("HTTPSProxy", s("127.0.0.1")),
        ("HTTPSPort", n(10809)),
        ("SOCKSEnable", n(1)),
        ("SOCKSProxy", s("127.0.0.1")),
        ("SOCKSPort", n(10808)),
        (
            "ExceptionsList",
            MacVal::List(vec![
                "*.local".into(),
                "169.254/16".into(),
                "192.168.0.0/16".into(),
            ]),
        ),
        ("ExcludeSimpleHostnames", n(1)),
    ]);
    let c = parse_macos(&d).unwrap();
    assert_eq!(c.source, Source::Macos);
    assert_eq!(
        c.primary().unwrap().display(),
        "http://127.0.0.1:10809",
        "http preferred over socks"
    );
    for h in ["printer.local", "169.254.3.4", "192.168.1.1", "nas"] {
        assert!(c.for_target(h, true).is_none(), "{h}");
    }
    assert!(c.for_target("api.deepgram.com", true).is_some());
    // socks only
    let d = mac(vec![
        ("SOCKSEnable", n(1)),
        ("SOCKSProxy", s("127.0.0.1")),
        ("SOCKSPort", n(10808)),
    ]);
    let c = parse_macos(&d).unwrap();
    assert_eq!(
        c.for_target("openrouter.ai", true).unwrap().scheme,
        Scheme::Socks5
    );
    // disabled / incomplete
    assert!(parse_macos(&mac(vec![
        ("HTTPEnable", n(0)),
        ("HTTPProxy", s("h")),
        ("HTTPPort", n(1))
    ]))
    .is_none());
    assert!(parse_macos(&mac(vec![("HTTPEnable", n(1)), ("HTTPProxy", s("h"))])).is_none());
    assert!(parse_macos(&HashMap::new()).is_none());
}

fn env(pairs: &[(&str, &str)]) -> Option<Config> {
    env_from(
        &pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    )
}

#[test]
fn env_precedence_and_case() {
    assert!(env(&[]).is_none());
    let c = env(&[
        ("HTTPS_PROXY", "http://a:1"),
        ("HTTP_PROXY", "http://b:2"),
        ("ALL_PROXY", "socks5://c:3"),
    ])
    .unwrap();
    assert_eq!(c.for_target("x.com", true).unwrap().display(), "http://a:1");
    assert_eq!(c.for_target("x.com", false).unwrap().display(), "http://b:2");
    let c = env(&[("ALL_PROXY", "socks5://c:3")]).unwrap();
    assert_eq!(c.for_target("x.com", true).unwrap().display(), "socks5://c:3");
    // lower case names
    let c = env(&[("https_proxy", "127.0.0.1:9")]).unwrap();
    assert_eq!(c.source, Source::Env);
    assert_eq!(
        c.for_target("x.com", true).unwrap().display(),
        "http://127.0.0.1:9"
    );
    // an https target falls back to the http proxy and vice versa
    let c = env(&[("HTTP_PROXY", "http://b:2")]).unwrap();
    assert!(c.for_target("x.com", true).is_some());
    // garbage is ignored
    assert!(env(&[("HTTPS_PROXY", "ftp://x:1")]).is_none());
}

#[test]
fn bypass_rules() {
    let c = env(&[
        ("HTTPS_PROXY", "http://p:1"),
        ("NO_PROXY", "example.com, .internal.test,10.0.0.0/8,*.lan"),
    ])
    .unwrap();
    for h in [
        "example.com",
        "api.example.com",
        "x.internal.test",
        "internal.test",
        "10.9.8.7",
        "box.lan",
    ] {
        assert!(c.for_target(h, true).is_none(), "{h}");
    }
    for h in ["notexample.com", "example.org", "11.0.0.1", "lan"] {
        assert!(c.for_target(h, true).is_some(), "{h}");
    }
    let star = env(&[("HTTPS_PROXY", "http://p:1"), ("NO_PROXY", "*")]).unwrap();
    assert!(star.for_target("openrouter.ai", true).is_none());
}

#[test]
fn loopback_always_bypasses_even_a_manual_proxy() {
    let c = env(&[("HTTPS_PROXY", "http://p:1")]).unwrap();
    for h in [
        "localhost",
        "LOCALHOST",
        "127.0.0.1",
        "127.8.0.1",
        "::1",
        "[::1]",
        "app.localhost",
    ] {
        assert!(
            c.for_target(h, true).is_none() && c.for_target(h, false).is_none(),
            "{h}"
        );
    }
    assert!(Bypass::is_loopback("localhost."));
    assert!(!Bypass::is_loopback("127.0.0.1.example.com"));
}

#[test]
fn url_splitting() {
    assert_eq!(
        split_url("https://openrouter.ai/api/v1"),
        Some(("openrouter.ai".into(), true))
    );
    assert_eq!(
        split_url("wss://api.deepgram.com:443/v1"),
        Some(("api.deepgram.com".into(), true))
    );
    assert_eq!(
        split_url("http://user:pw@127.0.0.1:8080/x"),
        Some(("127.0.0.1".into(), false))
    );
    assert_eq!(split_url("http://[::1]:11434/api"), Some(("::1".into(), false)));
    assert_eq!(split_url("nonsense"), None);
}

#[test]
fn dialer_tunnels_through_http_connect_and_socks5() {
    use std::io::{Read, Write};
    let backend = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let baddr = backend.local_addr().unwrap();
    std::thread::spawn(move || {
        for c in backend.incoming() {
            let mut c = c.unwrap();
            let mut b = [0u8; 4];
            c.read_exact(&mut b).unwrap();
            assert_eq!(&b, b"ping");
            c.write_all(b"pong").unwrap();
        }
    });
    for kind in [mock::Kind::HttpConnect, mock::Kind::Socks5] {
        let p = mock::MockProxy::start(kind, baddr);
        let url = ProxyUrl::parse(&p.url()).unwrap();
        let mut s = connect_via(&url, "api.example.test", 443, std::time::Duration::from_secs(5)).unwrap();
        s.write_all(b"ping").unwrap();
        let mut r = [0u8; 4];
        s.read_exact(&mut r).unwrap();
        assert_eq!(&r, b"pong", "{kind:?}");
        assert_eq!(p.seen(), vec!["api.example.test:443".to_string()], "{kind:?}");
    }
    // a refused CONNECT is an error that names the proxy's answer
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut c, _) = l.accept().unwrap();
        let mut buf = [0u8; 512];
        let _ = c.read(&mut buf);
        c.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").unwrap();
    });
    let e = connect_via(
        &ProxyUrl::parse(&format!("http://{a}")).unwrap(),
        "x.test",
        443,
        std::time::Duration::from_secs(5),
    )
    .unwrap_err();
    assert!(e.to_string().contains("403"), "{e}");
}
