//! TTFT of live asks with the real key: `OPENROUTER_API_KEY=... cargo run --release -p lowk-llm --example ttft [N]`.
//! Compares the legacy unpinned slug with the shipped default (version-pinned + probed provider).
//! Tiny prompts (~400 tokens), a few cents at most. The key is read from the environment, never printed.
use lowk_llm::*;
use lowk_types::*;
use std::time::Instant;

fn run(cfg: &LlmConfig, n: usize, label: &str) {
    let client = OpenAiClient::new(cfg.clone());
    let b = PromptBuilder::new(&cfg.model);
    let mut ms: Vec<u128> = vec![];
    for i in 0..n {
        let mut l = b.new_ledger();
        let line = TranscriptLine {
            seq: 1,
            stream: Stream::System,
            offset_s: 65,
            text: format!("[{i}-{}] А что значит EBITDA в вашем отчёте за квартал?", Ulid::new()),
            is_final: true,
        };
        let t = b.build(&BuildInput {
            instruction: "Кратко ответь на вопрос.",
            transcript: std::slice::from_ref(&line),
            ..Default::default()
        });
        l.append(t.message);
        let t0 = Instant::now();
        let mut first = None;
        let o = ChatOptions { max_tokens: Some(60), ..ChatOptions::default() };
        let r = client.complete(&l, &o, &mut |_| { first.get_or_insert_with(|| t0.elapsed().as_millis()); }, &|| true);
        match (r, first) {
            (Ok(_), Some(f)) => ms.push(f),
            (r, _) => eprintln!("{label} run {i} failed: {:?}", r.err().map(|e| e.code)),
        }
    }
    ms.sort();
    if ms.is_empty() { println!("{label}: no samples"); return; }
    let p = |q: f32| ms[((ms.len() as f32 - 1.0) * q).round() as usize];
    println!("{label}: n={} TTFT p50={} ms p95={} ms min={} max={}  all={ms:?}", ms.len(), p(0.5), p(0.95), ms[0], ms[ms.len() - 1]);
}

fn main() {
    let key = std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY");
    let n: usize = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(8);
    let mut s = LlmSettings::default();
    // before: the old default (unpinned slug, no provider pin)
    let mut old = LlmConfig::for_ask(&s, Some(key.clone()), None);
    old.model = LEGACY_LLM_MODEL.into();
    old.fallback_model = None;
    run(&old, n.min(4), "legacy slug, default routing");
    // after: pinned version + fastest provider (probe), as the app does once per key
    let cfg = LlmConfig::for_summary(&s, Some(key.clone()));
    match probe_fastest_provider(&cfg, &s.model, &ProbeOptions::default(), &|| true) {
        Ok(r) => {
            println!("probe: best={} samples={:?}", r.best, r.samples.iter().map(|x| (x.provider.clone(), x.ttft_ms)).collect::<Vec<_>>());
            s.pinned_provider = Some(r.best);
        }
        Err(e) => println!("probe failed: {e}"),
    }
    let live = LlmConfig::for_ask(&s, Some(key), Some(Ulid::new().to_string()));
    run(&live, n, "shipped default (pinned slug + provider)");
}
