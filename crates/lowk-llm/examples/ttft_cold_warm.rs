//! Cold vs warm first-Ask TTFT with the real key:
//! `OPENROUTER_API_KEY=... cargo run --release -p lowk-llm --example ttft_cold_warm [N]`.
//! Each trial builds a FRESH client (new connection pool, as at app start). "cold" sends the Ask
//! straight away; "warm" calls `OpenAiClient::warm()` first (as the app does at start / when a
//! recording starts) and then sends the same kind of Ask. Tiny prompts, a few cents at most.
//! The key is read from the environment and never printed.
use lowk_llm::*;
use lowk_types::*;
use std::time::Instant;

fn ask(client: &OpenAiClient, model: &str, tag: usize) -> Option<u128> {
    let b = PromptBuilder::new(model);
    let mut l = b.new_ledger();
    let line = TranscriptLine {
        seq: 1,
        stream: Stream::System,
        offset_s: 65,
        text: format!("[{tag}-{}] А что значит EBITDA в вашем отчёте за квартал?", Ulid::new()),
        is_final: true,
    };
    let t = b.build(&BuildInput {
        instruction: "Кратко ответь на вопрос.",
        transcript: std::slice::from_ref(&line),
        ..Default::default()
    });
    l.append(t.message);
    let o = ChatOptions {
        max_tokens: Some(30),
        ..ChatOptions::default()
    };
    let t0 = Instant::now();
    let mut first = None;
    let r = client.complete(
        &l,
        &o,
        &mut |_| {
            first.get_or_insert_with(|| t0.elapsed().as_millis());
        },
        &|| true,
    );
    if let Err(e) = r {
        eprintln!("ask failed: {:?}", e.code);
    }
    first
}

fn stats(label: &str, mut v: Vec<u128>) {
    v.sort();
    if v.is_empty() {
        println!("{label}: no samples");
        return;
    }
    println!("{label}: n={} min={} median={} max={} ms  all={v:?}", v.len(), v[0], v[v.len() / 2], v[v.len() - 1]);
}

fn main() {
    let key = std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY");
    let n: usize = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(4);
    let s = LlmSettings::default();
    let (mut cold, mut warm, mut warm_ms) = (vec![], vec![], vec![]);
    for i in 0..n {
        let cfg = LlmConfig::for_ask(&s, Some(key.clone()), Some(Ulid::new().to_string()));
        let c = OpenAiClient::new(cfg.clone());
        cold.extend(ask(&c, &cfg.model, i));
        let c = OpenAiClient::new(cfg.clone());
        match c.warm() {
            Ok(d) => warm_ms.push(d.as_millis()),
            Err(e) => eprintln!("warm failed: {:?}", e.code),
        }
        warm.extend(ask(&c, &cfg.model, 100 + i));
    }
    stats("cold first Ask (fresh client)", cold);
    stats("warm-up call itself", warm_ms);
    stats("warm first Ask (after warm())", warm);
}
