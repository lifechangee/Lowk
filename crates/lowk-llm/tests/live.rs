//! Opt-in live smoke test against OpenRouter:
//! `LOWK_LLM_LIVE=1 OPENROUTER_API_KEY=... cargo test -p lowk-llm --test live -- --nocapture`.
//! Costs a fraction of a cent. Never prints the key.
use lowk_llm::{CloudLlm, LlmConfig, ProviderKind};
use lowk_types::{CallId, LlmService, SummarizeInput};

const SAMPLE_RU: &str = include_str!("../../../fixtures/llm/sample_call_ru.txt");

#[test]
fn live_summary() {
    if std::env::var("LOWK_LLM_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipped: set LOWK_LLM_LIVE=1");
        return;
    }
    let key = std::env::var("OPENROUTER_API_KEY").ok().or_else(|| {
        // Fallback: a local key file (~/.lowk-test-keys) (KEY=value lines). Never printed.
        let home = std::env::var("HOME").ok()?;
        let text = std::fs::read_to_string(format!("{home}/.lowk-test-keys")).ok()?;
        text.lines()
            .find_map(|l| l.strip_prefix("OPENROUTER_API_KEY="))
            .map(|v| v.trim().trim_matches(['"', '\'']).to_string())
    });
    let key = key.expect("OPENROUTER_API_KEY (env or ~/.lowk-test-keys)");
    let mut cfg = LlmConfig::new(
        ProviderKind::OpenRouter,
        lowk_types::DEFAULT_OPENROUTER_URL,
        lowk_types::DEFAULT_LLM_MODEL_PINNED,
    )
    .with_key(Some(key));
    cfg.fallback_model = Some(lowk_llm::FALLBACK_MODEL.into());
    cfg.data_collection_deny = std::env::var("LOWK_LLM_LIVE_DENY").is_ok();
    let svc = CloudLlm::new(cfg);
    let input = SummarizeInput {
        call: CallId::new(),
        title: "Релиз и платежи".into(),
        transcript: SAMPLE_RU.into(),
        qa: vec![],
        languages: vec!["ru".into()],
        output_language: None,
        folder_instructions: None,
    };
    let mut first = None;
    let t0 = std::time::Instant::now();
    let s = svc
        .summarize(
            &input,
            &mut |_| {
                first.get_or_insert_with(|| t0.elapsed());
            },
            &|| true,
        )
        .expect("summary");
    println!("ttft {:?} total {:?} model {}", first, t0.elapsed(), s.model);
    println!("{}", s.md);
    assert!(!s.key_points.is_empty());
}

#[test]
fn live_probe() {
    if std::env::var("LOWK_LLM_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let key = std::env::var("OPENROUTER_API_KEY").ok().or_else(|| {
        std::fs::read_to_string(format!("{home}/.lowk-test-keys"))
            .ok()?
            .lines()
            .find_map(|l| l.strip_prefix("OPENROUTER_API_KEY="))
            .map(|v| v.trim().trim_matches(['"', '\'']).to_string())
    });
    let cfg = LlmConfig::new(
        ProviderKind::OpenRouter,
        lowk_types::DEFAULT_OPENROUTER_URL,
        lowk_types::DEFAULT_LLM_MODEL_PINNED,
    )
    .with_key(key);
    let r = lowk_llm::probe_fastest_provider(
        &cfg,
        lowk_types::DEFAULT_LLM_MODEL_PINNED,
        &lowk_llm::ProbeOptions {
            max_providers: 4,
            runs: 1,
        },
        &|| true,
    )
    .expect("probe");
    println!("best {} of {:?}", r.best, r.samples);
}
