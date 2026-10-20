# Session replay: nemotron-local (LocalTuning { right: 6, chunk_ms: 160, hang_ms: 480 }, correction=true)

| clip | CER live | CER corrected | lag p50 ms | lag p95 ms | n words |
|---|---|---|---|---|---|
| 01_me_q | 0.286 | 0.125 | 760 | 1683 | 150 |
| 01_them_q | 0.143 | 0.092 | 668 | 1501 | 56 |

**All clips**: word-to-screen p50 727 ms, p95 1594 ms (206 words); mean CER live 0.214, corrected 0.109

## Estimator audit (word-to-screen lag by bucket)

| bucket | n | p50 ms | p95 ms | p99 ms |
|---|---|---|---|---|
| last word of utterance (anchor exact) | 17 | 462 | 1381 | 1381 |
| pos <= 0.2 (anchor near start) | 40 | 1042 | 1616 | 1810 |
| pos 0.2..0.8 (estimated) | 118 | 676 | 1549 | 1952 |
| pos > 0.8 | 48 | 601 | 1683 | 2043 |
| utterance speech < 5 s | 30 | 547 | 1491 | 1501 |
| utterance speech 5..12 s | 83 | 798 | 1510 | 1810 |
| utterance speech >= 12 s | 93 | 734 | 1741 | 2679 |

Negative lags (estimator says word shown before spoken): 23 of 206

Engine diagnostics (last session): SttDiagnostics {
    engine: "nemotron-local",
    lookahead_ms: 640,
    streams: [
        StreamDiag {
            stream: Me,
            emit_lag: Percentiles {
                samples: 124,
                p50_ms: 90,
                p95_ms: 444,
                max_ms: 495,
            },
            final_lag: Percentiles {
                samples: 10,
                p50_ms: 71,
                p95_ms: 909,
                max_ms: 909,
            },
            backlog_ms: 103,
            restarts: 0,
        },
        StreamDiag {
            stream: System,
            emit_lag: Percentiles {
                samples: 41,
                p50_ms: 92,
                p95_ms: 325,
                max_ms: 451,
            },
            final_lag: Percentiles {
                samples: 7,
                p50_ms: 393,
                p95_ms: 556,
                max_ms: 556,
            },
            backlog_ms: 17,
            restarts: 0,
        },
    ],
    corrected: 15,
    correction_skipped: 0,
    correction_lag: Percentiles {
        samples: 17,
        p50_ms: 3495,
        p95_ms: 6626,
        max_ms: 6626,
    },
}

## Texts

### 01_me_q
live: Но я работал ПМ в Финтеч стартапе. В основном владел ланбордингом, делали продукт Discovery через юзер интервью с потом писал PRD и родмап на квартал. Да, был такой, мы запускали новый флоу Activation Right вырос на четыре процента, но Retension до тридцать упал начали сегментировать по кохортам. и понял, что пользователи из адс приходили без интент, мы сделали Абтест и поменяли Нор Стар Метрик на engaged Users. Э, использовал Райс, иногда айс, если данных мало и стекхолдер Алаййгмент это отдельная история с инженеринг лидом мы спорили по Тейчдеб. Сначала бы уточнил, кто юзер и какой джобс тубе дан GBT. Потом определял бы Примари метрик Например, Successful Walking Routines. и посмотрел был метрикс типа Crush Raid и Bachery Drein VP, скорее всего, оффлайн мапс. Ну, слушаю, но не делаю все, что они говорят, собираю дата из CRM, смотрю на паттерн с и потом уже извините, приносит к этому я немного сбился.

corrected: Я работал PM в Fintech стартапе, в основном владел онбордингом, делали product discovery через user interviews, потом писал PRD и roadmap на квартал. Да, был такой. Мы запускали новый Flow. ActivationRide вырос на 4%, но Retention до 30% упал. начали сегментировать по кохортам. и понял, что пользователи из Ads приходили без Intent. Мы сделали A-B-тест и поменяли NorthStarMetric на EngagedUsers. Использовал RISE, иногда ICE, если данных мало, и Stakeholder Alignment. Это отдельная история. С Engineering Lead мы спорили по Touch Depth. Сначала бы уточнил, кто юзер и какой jobs to be done, GBT. Потом определял бы primary metric Например, Successful Walking Routines и посмотрел бы Guard Tail Metrics типа Crash Rate и Battery Drain. MVP, скорее всего, Offline Maps. Ну, слушаю, но не делаю все, что они говорят. Собираю дата из CRM, смотрю на паттернс, и потом уже... Ой, извините, вернусь к этому, я немного сбился.

### 01_them_q
live: Привет, раз познакомиться. Ну расскажи коротко, чем занимался последние два года. А окей, ну расскажите с геметрики не сошлись с ожиданиями. Интересно, а как приоритизировал бэклог? М А как бы ты подошел к задаче улучшить Гугл Мапс для пешеходов? Хорошо, ну и последний вопрос Как работаешь с фидбеком от сейлс? Ничего, давай свяжемся на следующей неделе.

corrected: Привет, рад познакомиться. Ну, расскажи коротко, чем занимался последние два года. Окей. Ну расскажи кейс, где метрики не сошлись с ожиданиями. Интересно, а как приоритизировал Backlog? А как бы ты подошел к задаче улучшить Google Maps для пешеходов? Хорошо, ну и последний вопрос. Как работаешь с фидбэком от Sales? Ничего, давай свяжемся на следующей неделе.

## Ask probe (ask() every 2.3 s during replay)

n=42 ask() call ms p50 0.12 p95 0.18 max 0.66; open partials at ask time: 42, present in snapshot: 12
