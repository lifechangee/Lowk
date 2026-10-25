//! Summary and the latency probe through `LlmService` against the mock server.
use lowk_llm::mock::{MockServer, Reply};
use lowk_llm::{probe_fastest_provider, CloudLlm, LlmConfig, ProbeOptions, ProviderKind};
use lowk_types::{CallId, ErrorCode, LlmService, QaDigest, SummarizeInput};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const SAMPLE_RU: &str = include_str!("../../../fixtures/llm/sample_call_ru.txt");

fn cfg(s: &MockServer) -> LlmConfig {
    let mut c = LlmConfig::new(
        ProviderKind::OpenRouter,
        s.base_url(),
        lowk_types::DEFAULT_LLM_MODEL_PINNED,
    )
    .with_key(Some("sk-test".into()));
    c.retry_backoff = Duration::from_millis(5);
    c
}

fn input(tr: &str) -> SummarizeInput {
    SummarizeInput {
        call: CallId::new(),
        title: "Релиз".into(),
        transcript: tr.into(),
        qa: vec![QaDigest {
            question: "Когда демо?".into(),
            hint: "В четверг".into(),
            my_answer: Some("Думаю, в четверг после обеда".into()),
        }],
        languages: vec!["ru".into(), "en".into()],
        output_language: None,
        folder_instructions: Some("Клиент: Acme".into()),
    }
}

const RU_REPLY: &[&str] = &[
    "## Главное\n- Релиз сдвигается на неделю\n- Sandbox платёжного провайдера нестабилен\n",
    "## Решения\n- Релиз в пятницу, демо в четверг\n## Задачи\n- **Аня:** смета до среды\n",
    "## Вопросы ко мне\n- **Когда демо клиенту?** → Мой ответ: в четверг после обеда\n- **Кто тестирует платежи?** → Мой ответ: —\n",
];

#[test]
fn summary_golden_ru_streams_and_parses() {
    let s = MockServer::scripted(vec![Reply::answer(RU_REPLY)]);
    let svc = CloudLlm::new(cfg(&s));
    let mut streamed = String::new();
    let sum = svc
        .summarize(&input(SAMPLE_RU), &mut |d| streamed.push_str(d), &|| true)
        .unwrap();
    assert_eq!(streamed, RU_REPLY.concat());
    assert_eq!(sum.key_points.len(), 2);
    assert_eq!(sum.decisions.len(), 1);
    assert_eq!(sum.tasks[0].who.as_deref(), Some("Аня"));
    assert_eq!(
        sum.questions[0].my_answer.as_deref(),
        Some("в четверг после обеда")
    );
    assert_eq!(sum.questions[1].my_answer, None);
    assert!(sum.md.starts_with("## Главное"));
    assert_eq!(sum.model, "mock-model");
    assert!(sum.generated_at.contains('T'));

    // golden prompt checks
    let j = s.chat_requests()[0].json();
    assert_eq!(j["model"], lowk_types::DEFAULT_LLM_MODEL_PINNED);
    assert_eq!(j["reasoning"]["enabled"], false);
    let sys = j["messages"][0]["content"].as_str().unwrap();
    assert!(sys.contains("entirely in Russian"));
    for h in [
        "## Главное",
        "## Решения",
        "## Задачи",
        "## Вопросы ко мне",
        "Мой ответ",
    ] {
        assert!(sys.contains(h), "{h}");
    }
    let user = j["messages"][1]["content"].as_str().unwrap();
    assert!(
        user.contains("<folder>\nКлиент: Acme") && user.contains("<transcript>") && user.contains("<qa>")
    );
    assert!(user.contains("Me answered: Думаю, в четверг после обеда"));
}

#[test]
fn summary_english_when_call_is_english() {
    let s = MockServer::scripted(vec![Reply::answer(&[
        "## Key points\n- Shipped\n## Decisions\n- none\n## Tasks\n- none\n## Questions to me\n- none\n",
    ])]);
    let svc = CloudLlm::new(cfg(&s));
    let mut i = input("[00:00:01] **Me:** We shipped the release today and everything went fine.");
    i.languages = vec!["en".into()];
    i.qa.clear();
    i.folder_instructions = None;
    let sum = svc.summarize(&i, &mut |_| {}, &|| true).unwrap();
    assert_eq!(sum.key_points, ["Shipped"]);
    let sys = s.chat_requests()[0].json()["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(sys.contains("entirely in English") && sys.contains("## Questions to me"));
}

#[test]
fn long_transcript_is_map_reduced() {
    let long: String = (0..400)
        .map(|i| {
            format!(
                "[00:{:02}:00] **Я:** строка номер {i} про бюджет и сроки релиза\n",
                i % 60
            )
        })
        .collect();
    let s = MockServer::start(|req, i| {
        let sys = req.json()["messages"][0]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if sys.contains("Extract facts") {
            Reply::answer(&[&format!("- факт из части {i}")])
        } else {
            Reply::answer(RU_REPLY)
        }
    });
    let mut c = cfg(&s);
    c.summary_part_chars = 5_000;
    let svc = CloudLlm::new(c);
    let mut streamed = String::new();
    let sum = svc
        .summarize(&input(&long), &mut |d| streamed.push_str(d), &|| true)
        .unwrap();
    let reqs = s.chat_requests();
    let maps = reqs.iter().filter(|r| r.body.contains("Extract facts")).count();
    assert!(maps >= 3, "{maps} map calls");
    assert_eq!(reqs.len(), maps + 1);
    let last = reqs.last().unwrap().json();
    let user = last["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("<notes>") && user.contains("--- part 1 (") && !user.contains("строка номер 1 "));
    assert_eq!(streamed, RU_REPLY.concat(), "only the final reduce is streamed");
    assert_eq!(sum.tasks.len(), 1);
}

#[test]
fn summary_errors_surface_as_error_codes() {
    let svc = CloudLlm::new(LlmConfig::new(
        ProviderKind::OpenRouter,
        "http://127.0.0.1:1/api/v1",
        "m",
    ));
    assert_eq!(
        svc.summarize(&input("x"), &mut |_| {}, &|| true)
            .unwrap_err()
            .code,
        ErrorCode::ApiKeyMissing
    );

    let s = MockServer::scripted(vec![Reply::answer(&["I'm sorry, I can't help with that."])]);
    let svc = CloudLlm::new(cfg(&s));
    assert_eq!(
        svc.summarize(&input("hello there"), &mut |_| {}, &|| true)
            .unwrap_err()
            .code,
        ErrorCode::LlmBadResponse
    );

    let s = MockServer::scripted(vec![Reply::json(
        402,
        r#"{"error":{"message":"insufficient credits"}}"#,
    )]);
    let e = CloudLlm::new(cfg(&s))
        .summarize(&input("hello there"), &mut |_| {}, &|| true)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::LlmProvider);

    let svc = CloudLlm::new(cfg(&MockServer::scripted(vec![Reply::answer(&["x"])])));
    let mut i = input("");
    i.qa.clear();
    assert_eq!(
        svc.summarize(&i, &mut |_| {}, &|| true).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
}

#[test]
fn summary_cancel() {
    let s = MockServer::scripted(vec![Reply::stall_after(&["## Главное\n- a\n"])]);
    let svc = CloudLlm::new(cfg(&s));
    let cancel = AtomicBool::new(false);
    let r = svc.summarize(
        &input("привет мир"),
        &mut |_| cancel.store(true, Ordering::SeqCst),
        &|| !cancel.load(Ordering::SeqCst),
    );
    assert_eq!(r.unwrap_err().code, ErrorCode::LlmCancelled);
}

#[test]
fn probe_picks_fastest_provider() {
    let s = MockServer::start(|req, _| {
        if req.method == "GET" {
            return Reply::json(
                200,
                r#"{"data":{"endpoints":[{"tag":"slow","provider_name":"Slow"},{"tag":"fast/fp8","provider_name":"Fast"},{"tag":"dead","provider_name":"Dead"}]}}"#,
            );
        }
        let order = req.json()["provider"]["order"][0]
            .as_str()
            .unwrap_or("")
            .to_string();
        match order.as_str() {
            "slow" => Reply::Sse {
                pieces: lowk_llm::mock::sse_answer(&["ok"]),
                piece_delay: Duration::from_millis(250),
                hang_after: false,
            },
            "fast/fp8" => Reply::answer(&["ok"]),
            _ => Reply::json(500, "{}"),
        }
    });
    let r = probe_fastest_provider(
        &cfg(&s),
        "deepseek/deepseek-v4.1-flash-1017",
        &ProbeOptions {
            max_providers: 6,
            runs: 1,
        },
        &|| true,
    )
    .unwrap();
    assert_eq!(r.best, "fast/fp8");
    assert_eq!(r.samples.len(), 3);
    assert!(r
        .samples
        .iter()
        .find(|s| s.provider == "dead")
        .unwrap()
        .ttft_ms
        .is_none());
    assert!(s.requests().iter().any(|q| q
        .path
        .contains("models/deepseek/deepseek-v4.1-flash-1017/endpoints")));
    // probing needs a key
    let mut c = cfg(&s);
    c.api_key = None;
    assert_eq!(
        probe_fastest_provider(&c, "m", &ProbeOptions::default(), &|| true)
            .unwrap_err()
            .code,
        ErrorCode::ApiKeyMissing
    );
}

/// A 2 h Russian lecture (~22k words, RU with English terms), stamped like the vault's call files.
fn lecture_2h() -> String {
    let mut s = String::new();
    let mut words = 0;
    let mut t = 0u32;
    let mut i = 0;
    while t < 2 * 3600 {
        let line = format!(
            "[{:02}:{:02}:{:02}] **Собеседник:** тема {i}: {}\n",
            t / 3600,
            t / 60 % 60,
            t % 60,
            "обсуждаем pipeline, latency и deadline релиза, затем backlog и retention по кварталам и ещё немного про sandbox ".repeat(4)
        );
        words += line.split_whitespace().count();
        s.push_str(&line);
        t += 25; // ~ 3 words/s of lecture
        i += 1;
    }
    assert!(words > 15_000, "{words}");
    s
}

fn long_responder(req: &lowk_llm::mock::RecordedRequest, i: usize) -> Reply {
    let sys = req.json()["messages"][0]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if sys.contains("Extract facts") {
        // a verbose map answer so the notes force an extra merge level
        Reply::answer(&[&format!(
            "- факт {i}: {}",
            "термин pipeline и latency ".repeat(40)
        )])
    } else if sys.contains("Merge these consecutive notes") {
        Reply::answer(&[&format!("- слитые факты {i}: pipeline, latency")])
    } else {
        Reply::answer(RU_REPLY)
    }
}

#[test]
fn two_hour_lecture_is_chunked_by_time_with_progress() {
    let tr = lecture_2h();
    let s = MockServer::start(long_responder);
    let mut c = cfg(&s);
    c.summary_part_secs = 1200; // 20 min slices
    let svc = CloudLlm::new(c);
    let (mut progress, mut streamed) = (vec![], String::new());
    let sum = svc
        .summarize_with_progress(
            &input(&tr),
            &mut |d| streamed.push_str(d),
            &mut |a, b| progress.push((a, b)),
            &|| true,
        )
        .unwrap();
    let reqs = s.chat_requests();
    let maps: Vec<_> = reqs.iter().filter(|r| r.body.contains("Extract facts")).collect();
    assert_eq!(maps.len(), 6, "2 h / 20 min");
    // every slice is labelled with its recording range and keeps the language/terms rules
    let first = maps[0].json()["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(first.contains("part 1 of 6 (00:00:00-00:20:00)") && first.contains("in Russian"));
    assert!(first.contains("English terms exactly as spoken"));
    // no map request exceeds the configured part size
    assert!(maps.iter().all(|r| r.body.chars().count() < 70_000));
    let last = reqs.last().unwrap().json();
    let sys = last["messages"][0]["content"].as_str().unwrap();
    let user = last["messages"][1]["content"].as_str().unwrap();
    assert!(sys.contains("entirely in Russian") && sys.contains("ONE long recording"));
    assert!(
        user.contains("<notes>") && user.contains("--- part 6 (01:42:"),
        "{user:.300}"
    );
    assert!(
        !user.contains("<transcript>"),
        "the raw transcript never reaches the reduce call"
    );
    assert_eq!(streamed, RU_REPLY.concat(), "only the final reduce is streamed");
    assert_eq!(sum.key_points.len(), 2);
    // progress is monotonic, ends on the final step, and never exceeds its total
    assert!(progress.windows(2).all(|w| w[1].0 == w[0].0 + 1));
    let (step, total) = *progress.last().unwrap();
    assert_eq!(step as usize, reqs.len());
    assert!(step <= total && progress.iter().all(|p| p.0 <= p.1));
}

#[test]
fn huge_notes_are_merged_hierarchically_before_the_final_call() {
    let tr = lecture_2h();
    let s = MockServer::start(long_responder);
    let mut c = cfg(&s);
    c.summary_part_secs = 600; // 12 slices of verbose notes (~1.1k chars each)
    c.summary_notes_chars = 4_000;
    let svc = CloudLlm::new(c);
    svc.summarize(&input(&tr), &mut |_| {}, &|| true).unwrap();
    let reqs = s.chat_requests();
    let merges = reqs
        .iter()
        .filter(|r| r.body.contains("Merge these consecutive notes"))
        .count();
    assert!(merges >= 2, "{merges} merge calls");
    let user = reqs.last().unwrap().json()["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        user.contains("--- group 1.") && !user.contains("факт 0:"),
        "final call sees merged notes only"
    );
    assert!(user.chars().count() < 8_000, "{}", user.chars().count());
}

#[test]
fn short_call_is_one_request_with_trivial_progress() {
    let s = MockServer::scripted(vec![Reply::answer(RU_REPLY)]);
    let svc = CloudLlm::new(cfg(&s));
    let mut p = vec![];
    svc.summarize_with_progress(
        &input(SAMPLE_RU),
        &mut |_| {},
        &mut |a, b| p.push((a, b)),
        &|| true,
    )
    .unwrap();
    assert_eq!(s.chat_requests().len(), 1);
    assert_eq!(p, [(1, 1)]);
}
