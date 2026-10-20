# OpenRouter: streaming / real-time STT для Lowk

Дата: 2026-10-21. Источники на момент проверки.

## Короткий ответ

OpenRouter **не предоставляет** streaming STT: нет WebSocket, realtime-API или потоковой подачи аудио с частичными транскриптами. Есть только пакетная транскрибация (`POST /api/v1/audio/transcriptions`), которая принимает готовый фрагмент аудио и возвращает текст. Для субтитров <1 с этого недостаточно.

## Четыре сценария

| Вариант | Есть на OpenRouter? | Детали |
|---|---|---|
| (a) Пакетная транскрибация (файл -> текст) | Да | `POST https://openrouter.ai/api/v1/audio/transcriptions`, JSON с base64 (`input_audio.data`, `format`) или multipart (`file`) совместимо с OpenAI SDK. Ответ: `text` + `usage` (seconds, tokens, cost). |
| (b) Аудио как вход чат-LLM (gpt-4o-audio и т.п.) | Да | `POST /api/v1/chat/completions`, content part `input_audio`, только base64 (URL не поддерживаются). Не STT-специфично, дороже и медленнее. |
| (c) Стриминг вывода (SSE) vs стриминг входа аудио | Только SSE для текста | `stream: true` даёт токены текста по SSE. Стриминга входного аудио в документации нет. Для STT-эндпоинта streaming не упоминается. |
| (d) Realtime / WebSocket аудио API | Нет | Ни в docs, ни в анонсах не найдено. Сторонний гайд по GPT-Realtime-2 отмечает агрегаторы (включая OpenRouter) как неподдерживаемые для Realtime API (вторичный источник, может устареть). |

## Зачем в каталоге Whisper и другие speech-модели

Модели с модальностью `transcription` доступны через тот же пакетный эндпоинт. Это единый биллинг и маршрутизация к разным провайдерам. Им не нужно поддерживать WebSocket: каждая модель обрабатывает один загруженный фрагмент. Официальная коллекция `speech-to-text-models` содержит 25 моделей (Whisper, GPT-4o Transcribe, Deepgram Nova-3, ElevenLabs Scribe, AssemblyAI, NVIDIA Nemotron/Parakeet и др.). Модель `nvidia/nemotron-3.5-asr-streaming-multilingual-0.6b` имеет "streaming" в названии, но на её странице описан только однократный POST целого WAV-файла, без потоковых сессий.

## Ограничения пакетного API

- Таймаут на стороне провайдера: 60 с на запрос. Длинные записи нужно резать.
- Multipart: лимит 25 МБ. Base64 JSON больше 25 МБ принимают только OpenAI и Groq.
- Формат `text`, `srt`, `vtt` для multipart возвращает 400. Поддерживаются `json` и `verbose_json`.
- Лимита длительности аудио не указано. Ограничения rate limit: см. https://openrouter.ai/docs/faq#how-are-rate-limits-calculated.
- Нет задокументированных данных о latency.

## Цены (Whisper и аналоги, per second of audio)

| Модель | Цена | ~ в час |
|---|---|---|
| openai/whisper-1 | $0.0001/s | $0.36 |
| openai/whisper-large-v3 | $0.000008/s | $0.029 |
| openai/whisper-large-v3-turbo | $0.000003/s | $0.011 |
| nvidia/parakeet-tdt-0.6b-v3 | $0.000025/s | $0.09 |
| deepgram/nova-3 | от $0.000072/s | от $0.26 |
| openai/gpt-4o-transcribe | $2.50/M in, $10/M out токенов | зависит от токенов |

Примечание: на странице whisper-1 описание говорит "per minute", но цена в поле указана $0.0001/s. Перед заявкой в бюджет проверить на странице модели.

## Viability для live-субтитров <1 с

- Чанки по 1–2 с: сам сбор чанка даёт задержку 1–2 с до начала инференса. Плюс сеть, загрузка, инференс и ответ. Итог 2–4 с и больше. <1 с недостижимо.
- Чанки перекрываются, слова режутся на границах, контекст между запросами теряется. Нет partial-результатов, нельзя стабилизировать текст.
- Стоимость при чанках 2 с: биллинг по секундам, округление до секунды. Оплата примерно той же длины аудио, плюс накладные HTTP-вызовы. При 1 часе аудио с whisper-1 это около $0.36, с large-v3-turbo около $0.01. Стоимость не проблема, проблема latency.
- Вывод: чанкованный batch через OpenRouter подходит для записи/постобработки и субтитров с задержкой несколько секунд, но не для <1 с.

## Рекомендация

Для live-субтитров (<1 с) использовать streaming STT напрямую у провайдера с WebSocket (например Deepgram Nova-3 streaming, AssemblyAI Universal streaming, ElevenLabs Scribe realtime, OpenAI Realtime transcription), а не через OpenRouter. OpenRouter можно оставить для пакетной транскрибации после встречи, суммаризации и LLM-обработки.

## Источники

- OpenRouter STT guide: https://openrouter.ai/docs/guides/overview/multimodal/stt
- OpenRouter audio input guide: https://openrouter.ai/docs/guides/overview/multimodal/audio
- Коллекция STT-моделей: https://openrouter.ai/collections/speech-to-text-models
- Страница whisper-1 и цены: https://openrouter.ai/openai/whisper-1 , https://openrouter.ai/openai/whisper-1/pricing
- Nemotron streaming (названо "streaming", но пакетный POST): https://openrouter.ai/nvidia/nemotron-3.5-asr-streaming-multilingual-0.6b
- Анонс (вторичный): https://www.dsebastien.net/openrouter-adds-unified-audio-transcription/ (опубликовано 20 Aug 2026, обновлено 04 Oct 2026)
- Документация OpenRouter (общая): https://openrouter.ai/docs
- Сторонний гайд по Realtime API: https://webscraft.org/blog/gptrealtime2-tehnichniy-gid-websocket-api-pidklyuchennya-i-prikladi-kodu?lang=en

Не удалось проверить: страница анонса на openrouter.ai (404 по зеркалу), API-референс для create-transcription (404), официальный changelog и Discord/GitHub.
