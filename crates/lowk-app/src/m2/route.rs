//! Where an LLM request goes, and whether it may go there (BRIEF: local-only folders never use
//! the cloud). Pure functions over settings, so the policy is unit-tested without a network.
//!
//! * A folder with `allow_cloud = false` is served by a local model only: the configured provider
//!   when it is local (Ollama / LM Studio / llama-server / a loopback custom URL), else
//!   `llm.local_base_url` + `llm.local_model`. With neither, the request is refused with
//!   `LocalOnlyBlocked` before anything is retrieved or sent.
//! * Everything else uses the configured provider; OpenRouter needs the BYOK key (`ApiKeyMissing`).
use lowk_llm::{LlmConfig, ProviderKind};
use lowk_types::{AppError, ErrorCode, LlmProvider, LlmSettings, Result};

#[derive(Clone, Debug)]
pub enum Purpose {
    /// Live answer (`settings.llm.model`); `session_id` keeps OpenRouter's sticky routing per call.
    Ask { session_id: Option<String> },
    /// Summary and tags (`settings.llm.summary_model`, reasoning off).
    Summary,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub cfg: LlmConfig,
    /// True when the request leaves this machine (retrieval then withholds local-only folders).
    pub cloud: bool,
}

/// Host of an `http(s)://host[:port]/path` URL, lower-case, without brackets.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let auth = rest.split(['/', '?', '#']).next()?;
    let auth = auth.rsplit('@').next()?;
    let host = if let Some(v6) = auth.strip_prefix('[') {
        v6.split(']').next()?.to_string()
    } else {
        auth.split(':').next()?.to_string()
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Loopback, private LAN and `.local` hosts: nothing that leaves the machine's own network.
pub fn is_local_url(url: &str) -> bool {
    let Some(h) = host_of(url) else { return false };
    if h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") || h == "::1" {
        return true;
    }
    let p: Vec<u8> = h.split('.').filter_map(|x| x.parse().ok()).collect();
    if p.len() != 4 || h.split('.').count() != 4 {
        return false;
    }
    p[0] == 127 || p[0] == 10 || (p[0] == 192 && p[1] == 168) || (p[0] == 172 && (16..=31).contains(&p[1]))
}

/// The configured provider runs on this machine / network.
pub fn provider_is_local(s: &LlmSettings) -> bool {
    match s.provider {
        LlmProvider::Openrouter => false,
        LlmProvider::Ollama | LlmProvider::Lmstudio | LlmProvider::LlamaServer => true,
        LlmProvider::Custom => is_local_url(&s.base_url),
    }
}

fn base(s: &LlmSettings, key: Option<String>, purpose: &Purpose) -> LlmConfig {
    match purpose {
        Purpose::Ask { session_id } => LlmConfig::for_ask(s, key, session_id.clone()),
        Purpose::Summary => LlmConfig::for_summary(s, key),
    }
}

/// Picks the endpoint for one request. `local_only` = the folder has `allow_cloud = false`.
pub fn resolve(s: &LlmSettings, local_only: bool, key: Option<String>, purpose: &Purpose) -> Result<Route> {
    if provider_is_local(s) {
        let mut cfg = base(s, None, purpose);
        // no cloud fallback model, no key sent to a local server
        cfg.fallback_model = None;
        if matches!(purpose, Purpose::Summary) {
            // the summary slug names a cloud model; a local server answers with the one it serves
            cfg.model = s.model.clone();
        }
        return Ok(Route { cfg, cloud: false });
    }
    if local_only {
        let url = s.local_base_url.trim();
        let model = s.local_model.trim();
        if url.is_empty() || model.is_empty() || !is_local_url(url) {
            return Err(
                AppError::new(ErrorCode::LocalOnlyBlocked).with_detail(if url.is_empty() {
                    "no local LLM configured"
                } else if !is_local_url(url) {
                    "the local LLM URL is not a local address"
                } else {
                    "no local model name configured"
                }),
            );
        }
        let mut cfg = LlmConfig::new(ProviderKind::Custom, url, model);
        cfg.reasoning = false;
        if let Purpose::Ask { session_id } = purpose {
            cfg.session_id = session_id.clone();
        }
        return Ok(Route { cfg, cloud: false });
    }
    let kind: ProviderKind = s.provider.into();
    if kind == ProviderKind::OpenRouter && key.as_deref().is_none_or(|k| k.trim().is_empty()) {
        return Err(AppError::new(ErrorCode::ApiKeyMissing));
    }
    Ok(Route {
        cfg: base(s, key, purpose),
        cloud: true,
    })
}

/// Models known to read images, by slug. OpenRouter's model metadata (`architecture.input_modalities`)
/// is not fetched by the app, so this is the capability source; a model not matched here is treated
/// as text-only and image Asks are routed to `llm.vision_model` instead.
pub fn model_has_vision(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    let name = m.rsplit('/').next().unwrap_or(&m);
    if m.starts_with("anthropic/claude-")
        || m.starts_with("google/gemini")
        || m.starts_with("openai/gpt-4o")
        || m.starts_with("openai/gpt-4.1")
        || m.starts_with("openai/gpt-5")
        || m.starts_with("openai/o4")
        || m.starts_with("x-ai/grok-4")
        || m.starts_with("meta-llama/llama-4")
    {
        return true;
    }
    [
        "vision",
        "-vl",
        "llava",
        "pixtral",
        "minicpm-v",
        "gemma3",
        "gemma-3",
        "mistral-small3",
        "mistral-small-3",
        "qwen2.5vl",
        "qwen3-vl",
        "moondream",
        "bakllava",
    ]
    .iter()
    .any(|k| name.contains(k))
}

/// Picks the endpoint for an Ask that carries screenshots. Same policy as [`resolve`] plus:
/// * local provider / local-only folder: only a local vision model (`llm.local_vision_model`, or the
///   local model itself when its name is a known vision model); none = `ImageLocalOnly` (local-only
///   folder) or `NoVision`. Screenshots never reach the cloud from a local-only folder.
/// * cloud: the configured model when it reads images, else `llm.vision_model` (default
///   `DEFAULT_VISION_MODEL`); the upstream-provider pin of the text model does not apply to it.
pub fn resolve_vision(
    s: &LlmSettings,
    local_only: bool,
    key: Option<String>,
    session_id: Option<String>,
) -> Result<Route> {
    let purpose = Purpose::Ask {
        session_id: session_id.clone(),
    };
    if provider_is_local(s) {
        let model = if !s.local_vision_model.trim().is_empty() {
            s.local_vision_model.trim().to_string()
        } else if model_has_vision(&s.model) {
            s.model.clone()
        } else {
            return Err(AppError::new(ErrorCode::NoVision).with_detail("local model has no vision"));
        };
        let mut r = resolve(s, local_only, None, &purpose)?;
        r.cfg.model = model;
        return Ok(r);
    }
    if local_only {
        let url = s.local_base_url.trim();
        let model = if !s.local_vision_model.trim().is_empty() {
            s.local_vision_model.trim()
        } else if model_has_vision(&s.local_model) {
            s.local_model.trim()
        } else {
            ""
        };
        if url.is_empty() || !is_local_url(url) || model.is_empty() {
            return Err(AppError::new(ErrorCode::ImageLocalOnly)
                .with_detail("no local vision model for a local-only folder"));
        }
        let mut cfg = LlmConfig::new(ProviderKind::Custom, url, model);
        cfg.reasoning = false;
        cfg.session_id = session_id;
        return Ok(Route { cfg, cloud: false });
    }
    let mut r = resolve(s, false, key, &purpose)?;
    if !model_has_vision(&r.cfg.model) {
        let v = s.vision_model.trim();
        r.cfg.model = if v.is_empty() {
            lowk_types::DEFAULT_VISION_MODEL.into()
        } else {
            v.into()
        };
        r.cfg.pinned_provider = None;
        r.cfg.reasoning = false;
        r.cfg.fallback_model = (r.cfg.model != lowk_llm::FALLBACK_MODEL
            && r.cfg.kind == ProviderKind::OpenRouter)
            .then(|| lowk_llm::FALLBACK_MODEL.to_string());
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud() -> LlmSettings {
        LlmSettings::default()
    }

    #[test]
    fn local_hosts() {
        for u in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:8080/v1",
            "http://192.168.1.20:11434/v1",
            "http://10.0.0.5/v1",
            "http://172.20.1.1/v1",
            "http://studio.local:1234/v1",
        ] {
            assert!(is_local_url(u), "{u}");
        }
        for u in [
            "https://openrouter.ai/api/v1",
            "http://172.32.0.1/v1",
            "http://8.8.8.8/v1",
            "http://localhost.evil.com/v1",
            "http://127.0.0.1.evil.com/v1",
            "not a url",
            "",
        ] {
            assert!(!is_local_url(u), "{u}");
        }
    }

    #[test]
    fn local_only_folder_never_gets_a_cloud_route() {
        // cloud provider + local-only folder + no local LLM: refused, even with a valid key
        let e = resolve(
            &cloud(),
            true,
            Some("sk-or-x".into()),
            &Purpose::Ask { session_id: None },
        )
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::LocalOnlyBlocked);
        let e = resolve(&cloud(), true, Some("sk-or-x".into()), &Purpose::Summary).unwrap_err();
        assert_eq!(e.code, ErrorCode::LocalOnlyBlocked);
        // a non-local "local" URL is not a local LLM
        let mut s = cloud();
        s.local_base_url = "https://api.example.com/v1".into();
        s.local_model = "m".into();
        assert_eq!(
            resolve(&s, true, Some("k".into()), &Purpose::Summary)
                .unwrap_err()
                .code,
            ErrorCode::LocalOnlyBlocked
        );
        // model name missing
        s.local_base_url = "http://localhost:11434/v1".into();
        s.local_model = String::new();
        assert_eq!(
            resolve(&s, true, None, &Purpose::Summary).unwrap_err().code,
            ErrorCode::LocalOnlyBlocked
        );
        // configured: routes to the local server, no key, not cloud
        s.local_model = "gemma3".into();
        let r = resolve(
            &s,
            true,
            Some("sk-or-x".into()),
            &Purpose::Ask {
                session_id: Some("c".into()),
            },
        )
        .unwrap();
        assert!(!r.cloud);
        assert_eq!(r.cfg.base_url, "http://localhost:11434/v1");
        assert_eq!(r.cfg.model, "gemma3");
        assert!(
            r.cfg.api_key.is_none(),
            "the OpenRouter key never goes to a local server"
        );
        assert!(r.cfg.fallback_model.is_none());
    }

    #[test]
    fn local_provider_serves_every_folder_and_is_never_cloud() {
        let mut s = cloud();
        s.provider = LlmProvider::Ollama;
        s.base_url = "http://localhost:11434/v1".into();
        s.model = "qwen3".into();
        for local_only in [true, false] {
            let r = resolve(&s, local_only, None, &Purpose::Summary).unwrap();
            assert!(!r.cloud);
            assert_eq!(r.cfg.model, "qwen3");
            assert!(r.cfg.fallback_model.is_none());
        }
        // a Custom provider on a public host is a cloud route (and blocked for local-only folders)
        s.provider = LlmProvider::Custom;
        s.base_url = "https://api.example.com/v1".into();
        assert!(resolve(&s, false, None, &Purpose::Summary).unwrap().cloud);
        assert_eq!(
            resolve(&s, true, None, &Purpose::Summary).unwrap_err().code,
            ErrorCode::LocalOnlyBlocked
        );
    }

    #[test]
    fn cloud_route_needs_the_key_and_uses_the_pinned_model() {
        let e = resolve(&cloud(), false, None, &Purpose::Ask { session_id: None }).unwrap_err();
        assert_eq!(e.code, ErrorCode::ApiKeyMissing);
        let e = resolve(
            &cloud(),
            false,
            Some("  ".into()),
            &Purpose::Ask { session_id: None },
        )
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::ApiKeyMissing);
        let r = resolve(
            &cloud(),
            false,
            Some("k".into()),
            &Purpose::Ask {
                session_id: Some("c".into()),
            },
        )
        .unwrap();
        assert!(r.cloud);
        assert_eq!(r.cfg.model, lowk_types::DEFAULT_LLM_MODEL_PINNED);
        assert_eq!(r.cfg.session_id.as_deref(), Some("c"));
        assert!(!r.cfg.reasoning);
    }

    #[test]
    fn vision_capability_list() {
        assert!(model_has_vision("anthropic/claude-haiku-5.5"));
        assert!(model_has_vision("google/gemini-2.5-flash"));
        assert!(model_has_vision("qwen3-vl:8b"));
        assert!(model_has_vision("llama3.2-vision"));
        assert!(!model_has_vision("deepseek/deepseek-v4.1-flash-1017"));
        assert!(!model_has_vision("gemma2"));
    }

    #[test]
    fn non_vision_model_routes_to_the_vision_model() {
        let mut s = cloud();
        s.pinned_provider = Some("StubHost".into());
        let r = resolve_vision(&s, false, Some("k".into()), Some("c".into())).unwrap();
        assert!(r.cloud);
        assert_eq!(r.cfg.model, lowk_types::DEFAULT_VISION_MODEL);
        assert!(r.cfg.pinned_provider.is_none());
        s.vision_model = "openai/gpt-5-mini".into();
        assert_eq!(
            resolve_vision(&s, false, Some("k".into()), None)
                .unwrap()
                .cfg
                .model,
            "openai/gpt-5-mini"
        );
        // a vision-capable chat model answers images itself
        s.model = "google/gemini-2.5-flash".into();
        assert_eq!(
            resolve_vision(&s, false, Some("k".into()), None)
                .unwrap()
                .cfg
                .model,
            "google/gemini-2.5-flash"
        );
        assert_eq!(
            resolve_vision(&cloud(), false, None, None).unwrap_err().code,
            ErrorCode::ApiKeyMissing
        );
    }

    #[test]
    fn local_only_folder_never_sends_images_to_the_cloud() {
        let e = resolve_vision(&cloud(), true, Some("k".into()), None).unwrap_err();
        assert_eq!(e.code, ErrorCode::ImageLocalOnly);
        let mut s = cloud();
        s.local_base_url = "http://localhost:11434/v1".into();
        s.local_model = "qwen3".into(); // text-only
        assert_eq!(
            resolve_vision(&s, true, Some("k".into()), None).unwrap_err().code,
            ErrorCode::ImageLocalOnly
        );
        s.local_vision_model = "qwen3-vl".into();
        let r = resolve_vision(&s, true, Some("k".into()), None).unwrap();
        assert!(!r.cloud && r.cfg.api_key.is_none());
        assert_eq!(r.cfg.model, "qwen3-vl");
        // local provider without a vision model: NoVision
        let mut l = cloud();
        l.provider = LlmProvider::Ollama;
        l.base_url = "http://localhost:11434/v1".into();
        l.model = "qwen3".into();
        assert_eq!(
            resolve_vision(&l, false, None, None).unwrap_err().code,
            ErrorCode::NoVision
        );
    }
}
