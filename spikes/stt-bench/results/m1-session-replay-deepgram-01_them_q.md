# Session replay: deepgram (LocalTuning { right: 6, chunk_ms: 160, hang_ms: 480 }, correction=false)

| clip | CER live | CER corrected | lag p50 ms | lag p95 ms | n words |
|---|---|---|---|---|---|
| 01_them_q | 0.161 | 0.161 | 349 | 1431 | 53 |

**All clips**: word-to-screen p50 349 ms, p95 1431 ms (53 words); mean CER live 0.161, corrected 0.161

Engine diagnostics (last session): SttDiagnostics {
    engine: "deepgram",
    lookahead_ms: 0,
    streams: [
        StreamDiag {
            stream: Me,
            emit_lag: Percentiles {
                samples: 0,
                p50_ms: 0,
                p95_ms: 0,
                max_ms: 0,
            },
            final_lag: Percentiles {
                samples: 0,
                p50_ms: 0,
                p95_ms: 0,
                max_ms: 0,
            },
            backlog_ms: 0,
            restarts: 0,
        },
        StreamDiag {
            stream: System,
            emit_lag: Percentiles {
                samples: 30,
                p50_ms: 361,
                p95_ms: 1025,
                max_ms: 1331,
            },
            final_lag: Percentiles {
                samples: 1,
                p50_ms: 362,
                p95_ms: 362,
                max_ms: 362,
            },
            backlog_ms: 1303,
            restarts: 0,
        },
    ],
    corrected: 0,
    correction_skipped: 0,
    correction_lag: Percentiles {
        samples: 0,
        p50_ms: 0,
        p95_ms: 0,
        max_ms: 0,
    },
}

## Texts

### 01_them_q
live: Привет, рад познакомиться. Ну расскажи коротко, чем занимался последние 2 года. Окей. Ну, расскажи кейс, где метрики не сошлись с ожиданиями. Интересно, а как приоритезировал backlog? А как бы ты подошел задаче улучшить Google Maps для? Хорошо, ну и последний вопрос. Как работаешь с feedback at sales? Ничего, давай свяжемся на следующей неделе.

corrected: Привет, рад познакомиться. Ну расскажи коротко, чем занимался последние 2 года. Окей. Ну, расскажи кейс, где метрики не сошлись с ожиданиями. Интересно, а как приоритезировал backlog? А как бы ты подошел задаче улучшить Google Maps для? Хорошо, ну и последний вопрос. Как работаешь с feedback at sales? Ничего, давай свяжемся на следующей неделе.
