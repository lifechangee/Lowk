//! Local provider auto-detect: probes the usual loopback ports of Ollama (`/api/tags`), LM Studio and
//! llama-server (`/v1/models`) with a short timeout and lists their models. Plain HTTP on loopback:
//! no prompts, nothing leaves the machine. Parsers are pure; the probe takes its targets so tests
//! use their own mock servers instead of the machine's real ones.
use lowk_types::{LocalModel, LocalServer, LocalServerKind};
use serde_json::Value;
use std::time::Duration;

/// Per-request budget (connect + response); the three servers are probed in parallel.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTarget {
    pub kind: LocalServerKind,
    /// `http://127.0.0.1:11434` (no path).
    pub origin: String,
}

pub const DEFAULT_TARGETS: [(LocalServerKind, &str); 3] = [
    (LocalServerKind::Ollama, "http://127.0.0.1:11434"),
    (LocalServerKind::Lmstudio, "http://127.0.0.1:1234"),
    (LocalServerKind::LlamaServer, "http://127.0.0.1:8080"),
];

impl LocalTarget {
    pub fn defaults() -> Vec<LocalTarget> {
        DEFAULT_TARGETS
            .iter()
            .map(|(kind, o)| LocalTarget {
                kind: *kind,
                origin: (*o).to_string(),
            })
            .collect()
    }

    fn list_url(&self) -> String {
        let o = self.origin.trim_end_matches('/');
        match self.kind {
            LocalServerKind::Ollama => format!("{o}/api/tags"),
            _ => format!("{o}/v1/models"),
        }
    }

    fn base_url(&self) -> String {
        format!("{}/v1", self.origin.trim_end_matches('/'))
    }
}

/// Embedding-only models cannot answer chat requests.
fn is_embedding(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("embed") || n.contains("rerank")
}

fn dedup_chat_models(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        if !n.is_empty() && !is_embedding(&n) && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// Ollama `GET /api/tags`: `{"models":[{"name":"qwen3:8b",..}]}`. `None` = not an Ollama answer.
pub fn parse_ollama_tags(body: &str) -> Option<Vec<String>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let arr = v.get("models")?.as_array()?;
    Some(dedup_chat_models(arr.iter().filter_map(|m| {
        m.get("name")
            .or_else(|| m.get("model"))
            .and_then(Value::as_str)
            .map(str::to_string)
    })))
}

/// OpenAI-style `GET /v1/models`: `{"data":[{"id":"..."}]}` (LM Studio, llama-server).
pub fn parse_openai_models(body: &str) -> Option<Vec<String>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let arr = v.get("data")?.as_array()?;
    Some(dedup_chat_models(arr.iter().filter_map(|m| {
        m.get("id").and_then(Value::as_str).map(str::to_string)
    })))
}

fn probe_one(t: &LocalTarget, timeout: Duration, vision: &dyn Fn(&str) -> bool) -> Option<LocalServer> {
    let url = t.list_url();
    let proxy = lowk_platform::proxy::for_url(&url);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(proxy.and_then(|p| ureq::Proxy::new(&p).ok()))
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .into();
    let mut resp = agent.get(&url).call().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = resp.body_mut().read_to_string().ok()?;
    let names = match t.kind {
        LocalServerKind::Ollama => parse_ollama_tags(&body)?,
        _ => parse_openai_models(&body)?,
    };
    Some(LocalServer {
        kind: t.kind,
        base_url: t.base_url(),
        models: names
            .into_iter()
            .map(|name| LocalModel {
                vision: vision(&name),
                name,
            })
            .collect(),
    })
}

/// Probes all targets in parallel; returns the reachable ones in target order. `vision` flags models.
pub fn detect_local(
    targets: &[LocalTarget],
    timeout: Duration,
    vision: &(dyn Fn(&str) -> bool + Sync),
) -> Vec<LocalServer> {
    std::thread::scope(|s| {
        let hs: Vec<_> = targets
            .iter()
            .map(|t| s.spawn(move || probe_one(t, timeout, vision)))
            .collect();
        hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{MockServer, Reply};

    #[test]
    fn ollama_tags_parse_and_skip_embedding_models() {
        let b = r#"{"models":[{"name":"qwen3:8b","size":1},{"name":"nomic-embed-text:latest"},{"model":"gemma3:4b"},{"name":"qwen3:8b"}]}"#;
        assert_eq!(parse_ollama_tags(b).unwrap(), vec!["qwen3:8b", "gemma3:4b"]);
        assert_eq!(
            parse_ollama_tags(r#"{"models":[]}"#).unwrap(),
            Vec::<String>::new()
        );
        assert!(parse_ollama_tags("<html>").is_none());
        assert!(parse_ollama_tags(r#"{"data":[]}"#).is_none());
    }

    #[test]
    fn openai_models_parse() {
        let b = r#"{"object":"list","data":[{"id":"llama-3.2-3b"},{"id":"text-embedding-nomic"},{"id":"qwen2.5-vl-7b"}]}"#;
        assert_eq!(
            parse_openai_models(b).unwrap(),
            vec!["llama-3.2-3b", "qwen2.5-vl-7b"]
        );
        assert!(parse_openai_models(r#"{"models":[]}"#).is_none());
        assert!(parse_openai_models("").is_none());
    }

    fn json_server(path_ok: &'static str, body: &'static str) -> MockServer {
        MockServer::start(move |req, _| {
            if req.path.contains(path_ok) {
                Reply::json(200, body)
            } else {
                Reply::json(404, "{}")
            }
        })
    }

    #[test]
    fn probe_finds_servers_on_injected_origins_and_ignores_dead_ones() {
        let ollama = json_server(
            "/api/tags",
            r#"{"models":[{"name":"gemma3:4b"},{"name":"qwen3:8b"}]}"#,
        );
        let lms = json_server("/v1/models", r#"{"data":[{"id":"llama-3.2-3b"}]}"#);
        let wrong = json_server("/nothing", "{}"); // answers 404 on /v1/models
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let p = l.local_addr().unwrap().port();
            drop(l);
            format!("http://127.0.0.1:{p}")
        };
        let origin = |s: &MockServer| format!("http://127.0.0.1:{}", s.port());
        let targets = vec![
            LocalTarget {
                kind: LocalServerKind::Ollama,
                origin: origin(&ollama),
            },
            LocalTarget {
                kind: LocalServerKind::Lmstudio,
                origin: origin(&lms),
            },
            LocalTarget {
                kind: LocalServerKind::LlamaServer,
                origin: origin(&wrong),
            },
            LocalTarget {
                kind: LocalServerKind::LlamaServer,
                origin: dead,
            },
        ];
        let t0 = std::time::Instant::now();
        let found = detect_local(&targets, PROBE_TIMEOUT, &|n| n.contains("gemma3"));
        assert!(t0.elapsed() < Duration::from_secs(2));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].kind, LocalServerKind::Ollama);
        assert_eq!(found[0].base_url, format!("{}/v1", origin(&ollama)));
        assert_eq!(found[0].models.len(), 2);
        assert!(found[0].models[0].vision && !found[0].models[1].vision);
        assert_eq!(found[1].kind, LocalServerKind::Lmstudio);
        assert_eq!(found[1].models[0].name, "llama-3.2-3b");
    }

    #[test]
    fn probe_gives_up_on_a_hanging_server_within_the_timeout() {
        let hang = MockServer::start(|_, _| Reply::Hang);
        let t = vec![LocalTarget {
            kind: LocalServerKind::Lmstudio,
            origin: format!("http://127.0.0.1:{}", hang.port()),
        }];
        let t0 = std::time::Instant::now();
        assert!(detect_local(&t, PROBE_TIMEOUT, &|_| false).is_empty());
        assert!(t0.elapsed() < Duration::from_millis(1500));
    }

    #[test]
    fn defaults_cover_the_three_usual_ports() {
        let d = LocalTarget::defaults();
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].list_url(), "http://127.0.0.1:11434/api/tags");
        assert_eq!(d[1].list_url(), "http://127.0.0.1:1234/v1/models");
        assert_eq!(d[2].base_url(), "http://127.0.0.1:8080/v1");
    }
}
