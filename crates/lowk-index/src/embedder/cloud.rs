//! Cloud embeddings over an OpenAI-compatible `/embeddings` endpoint (OpenRouter by default).
//! The key is passed in by the app (never read from the keychain here, never logged).
use super::{truncate, Embedder};
use lowk_types::{AppError, ErrorCode, Result};
use std::time::Duration;

pub const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";

pub struct CloudEmbedder {
    base: String,
    key: String,
    model: String,
    dim: usize,
    id: String,
    agent: lowk_platform::proxy::ProxyCache<ureq::Agent>,
}

impl CloudEmbedder {
    /// `dim` is the size stored in the index (leading values of the vector are kept).
    pub fn new(base_url: &str, api_key: &str, model: &str, dim: usize) -> Self {
        Self {
            base: base_url.trim_end_matches('/').to_string(),
            key: api_key.to_string(),
            model: model.to_string(),
            dim,
            id: format!("cloud:{model}:{dim}"),
            agent: Default::default(),
        }
    }

    fn agent_for(&self, url: &str) -> ureq::Agent {
        self.agent.get(url, |proxy| {
            let tls = ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::Rustls)
                .unversioned_rustls_crypto_provider(super::tls_provider())
                .root_certs(ureq::tls::RootCerts::WebPki)
                .build();
            ureq::Agent::config_builder()
                .tls_config(tls)
                .proxy(proxy.and_then(|p| ureq::Proxy::new(p).ok()))
                .timeout_connect(Some(Duration::from_secs(15)))
                .timeout_global(Some(Duration::from_secs(60)))
                .http_status_as_error(false)
                .build()
                .into()
        })
    }
}

impl Embedder for CloudEmbedder {
    fn is_cloud(&self) -> bool {
        true
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn dim(&self) -> usize {
        self.dim
    }
    fn format_doc(&self, title: &str, text: &str) -> String {
        if title.trim().is_empty() {
            text.to_string()
        } else {
            format!("{title}\n{text}")
        }
    }
    fn format_query(&self, q: &str) -> String {
        q.to_string()
    }
    fn embed_docs(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if self.key.is_empty() {
            return Err(AppError::new(ErrorCode::ApiKeyMissing));
        }
        let body = serde_json::json!({ "model": self.model, "input": texts, "encoding_format": "float" });
        let url = format!("{}/embeddings", self.base);
        let mut resp = self
            .agent_for(&url)
            .post(&url)
            .header("Authorization", &format!("Bearer {}", self.key))
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|e| AppError::new(ErrorCode::Network).with_detail(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| AppError::new(ErrorCode::Network).with_detail(e.to_string()))?;
        if status == 401 || status == 403 {
            return Err(AppError::new(ErrorCode::ApiKeyMissing).with_param("status", status));
        }
        if !(200..300).contains(&status) {
            return Err(AppError::new(ErrorCode::EmbedFailed)
                .with_param("status", status)
                .with_detail(text.chars().take(200).collect::<String>()));
        }
        let v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AppError::new(ErrorCode::EmbedFailed).with_detail(e.to_string()))?;
        let data = v["data"]
            .as_array()
            .ok_or_else(|| AppError::new(ErrorCode::EmbedFailed).with_detail("no data array"))?;
        let mut rows: Vec<(usize, Vec<f32>)> = data
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let idx = d["index"].as_u64().map(|x| x as usize).unwrap_or(i);
                let e = d["embedding"].as_array()?;
                Some((
                    idx,
                    e.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect(),
                ))
            })
            .collect();
        rows.sort_by_key(|r| r.0);
        if rows.len() != texts.len() || rows.iter().any(|r| r.1.len() < self.dim) {
            return Err(AppError::new(ErrorCode::EmbedFailed).with_detail("unexpected embeddings shape"));
        }
        Ok(rows.into_iter().map(|r| truncate(r.1, self.dim)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn serve(status: u16, body: String) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            // Read the whole request (headers + body) before answering: replying early and
            // closing made the client see a connection reset instead of the status (flaky test).
            let mut req = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&req);
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text[..end]
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if req.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let _ = write!(
                s,
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        format!("http://127.0.0.1:{port}")
    }

    #[test]
    fn parses_and_truncates() {
        let url = serve(
            200,
            r#"{"data":[{"index":1,"embedding":[0,1,0,0]},{"index":0,"embedding":[1,0,0,0]}]}"#.into(),
        );
        let e = CloudEmbedder::new(&url, "k", "m", 2);
        let v = e.embed_docs(&["a".into(), "b".into()]).unwrap();
        assert_eq!(v, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    }

    #[test]
    fn maps_errors() {
        let url = serve(401, "{}".into());
        let e = CloudEmbedder::new(&url, "k", "m", 2);
        assert_eq!(
            e.embed_docs(&["a".into()]).unwrap_err().code,
            ErrorCode::ApiKeyMissing
        );
        let e = CloudEmbedder::new("http://127.0.0.1:1", "", "m", 2);
        assert_eq!(
            e.embed_docs(&["a".into()]).unwrap_err().code,
            ErrorCode::ApiKeyMissing
        );
        let url = serve(500, "boom".into());
        let e = CloudEmbedder::new(&url, "k", "m", 2);
        assert_eq!(
            e.embed_docs(&["a".into()]).unwrap_err().code,
            ErrorCode::EmbedFailed
        );
    }
}
