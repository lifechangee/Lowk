//! Offline OpenAI-compatible stub for `just qa-bundle`: point `llm.base_url` at it.
//! `lowk-llm-stub [--port N]` prints `LISTENING http://127.0.0.1:PORT/api/v1` and serves
//! canned streamed answers (summary, plain chat) until killed. QA tooling only.
use lowk_llm::mock::{MockServer, Reply};

const SUMMARY_RU: &[&str] = &[
    "## Главное\n- Обсудили бюджет и сроки\n- Релиз сдвигается на неделю\n",
    "## Решения\n- Сдвинуть релиз\n## Задачи\n- **Аня:** подготовить смету\n",
    "## Вопросы ко мне\n- **Когда релиз?** → Мой ответ: в пятницу\n",
];
const SUMMARY_EN: &[&str] = &[
    "## Key points\n- Budget and schedule discussed\n- Release slips by a week\n",
    "## Decisions\n- Move the release\n## Tasks\n- **Anna:** prepare the estimate\n",
    "## Questions to me\n- **When is the release?** → My answer: Friday\n",
];

/// A lecture gets its own sections instead of a meeting's (the transcript of the QA clip says «лекция» / «lecture»).
const SUMMARY_LECTURE_RU: &[&str] = &[
    "## Темы\n- Бюджет\n- Сроки релиза\n",
    "## Ключевые мысли\n- Релиз сдвигается на неделю\n## Термины и определения\n- **Релиз** — выпуск версии\n",
    "## Что спрашивали\n- Когда релиз?\n## Домашка / что выучить\n- Прочитать план релиза\n",
];

fn main() {
    let mut port = 0u16;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--port" {
            port = args.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        }
    }
    let server = MockServer::start_on(port, |req, _| {
        if req.method == "GET" {
            return Reply::json(
                200,
                r#"{"data":{"endpoints":[{"tag":"stub","provider_name":"Stub"}]}}"#,
            );
        }
        let body = req.body.as_str();
        if body.contains("post-call summaries") {
            // the user message (not the instructions, which name every kind of call) tells what was recorded
            let user = req.json()["messages"][1]["content"]
                .as_str()
                .unwrap_or("")
                .to_lowercase();
            if body.contains("Russian") && (user.contains("лекци") || user.contains("lecture")) {
                Reply::answer(SUMMARY_LECTURE_RU)
            } else if body.contains("Russian") {
                Reply::answer(SUMMARY_RU)
            } else {
                Reply::answer(SUMMARY_EN)
            }
        } else {
            Reply::answer(&["Stub ", "answer."])
        }
    });
    println!("LISTENING {}", server.base_url());
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
