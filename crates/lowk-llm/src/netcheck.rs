//! Settings -> Network "Check connection": one GET through the effective proxy.
use lowk_platform::proxy::{self, ProxyUrl};
use lowk_types::NetworkCheck;
use std::time::{Duration, Instant};

pub const CHECK_URL: &str = "https://openrouter.ai/api/v1/models";

/// `manual`: the proxy typed in the field (empty = whatever the system / saved override resolves to).
/// Any HTTP answer counts as reachable (`ok`), including 401/403; the status is reported so the user
/// can tell a blocked network (403 «Access denied») from a working one.
pub fn check(manual: &str, url: &str) -> NetworkCheck {
    let manual = manual.trim();
    let (proxy_url, via) = if manual.is_empty() {
        let p = proxy::for_url(url);
        let d = p.as_deref().and_then(ProxyUrl::parse).map(|p| p.display());
        (p, d)
    } else {
        match ProxyUrl::parse(manual) {
            Some(p) => (Some(p.to_url()), Some(p.display())),
            None => {
                return NetworkCheck {
                    ok: false,
                    status: None,
                    ms: 0,
                    via: None,
                    error: Some("bad proxy URL (use http://<host>:<port> or socks5://<host>:<port>)".into()),
                }
            }
        }
    };
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .unversioned_rustls_crypto_provider(crate::client::tls_provider())
        .root_certs(ureq::tls::RootCerts::WebPki)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(proxy_url.and_then(|p| ureq::Proxy::new(&p).ok()))
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let t0 = Instant::now();
    let r = agent.get(url).header("Accept", "application/json").call();
    let ms = t0.elapsed().as_millis().min(u32::MAX as u128) as u32;
    match r {
        Ok(resp) => NetworkCheck {
            ok: true,
            status: Some(resp.status().as_u16()),
            ms,
            via,
            error: None,
        },
        Err(e) => NetworkCheck {
            ok: false,
            status: None,
            ms,
            via,
            error: Some(short(&e.to_string())),
        },
    }
}

fn short(s: &str) -> String {
    s.chars().take(160).collect()
}
