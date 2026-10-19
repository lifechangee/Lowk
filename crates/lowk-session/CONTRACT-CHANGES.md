# Contract change proposals (WP7, for the architect)

None of these were applied to `lowk-types`; the session works without them.

1. **`AppEvent::MyAnswer { qa_id, text, closed }`** (+ event name `qa://my-answer`).
   The «Мой ответ» text grows as the user speaks; the UI needs it live. Today the
   session exposes it on a side channel, `Session::subscribe_qa()` -> `QaUpdate`
   (`qa_id`, `text`, `closed: Option<CloseReason>`), and `Session::qa_records()`.
   Move `QaUpdate`/`CloseReason`/`QaRecord` into `lowk-types` (specta derives).
2. **`AppEvent::Error`/stt failure event.** `SttSink::error` has no UI path; the session
   keeps them in `Session::stt_errors()` and logs. Needs an event (with `Stream`).
3. **`Vault::begin_call` must expose the call id** before `finish()`. The session's
   `CallStore::begin_call -> (CallId, Box<dyn CallRecorder>)` needs it; `lowk-app`
   writes a small adapter over `CallWriter` (`push`/`materialize`/`finish`).
4. **`SessionPhase`** is used as-is. `Paused` exists; `session_pause`/`session_resume`
   map to `Session::pause()`/`resume()`.
5. **`StartOpts`**: add `demo_speed: Option<f32>` if the UI wants a speed control
   (now `SessionDeps::demo_pace`).
6. `NewCall.started` is written as UTC RFC 3339 (`Z`); local offset needs a safe
   source from `lowk-platform`.
7. `EngineRegistry`/`SttEngineFactory` in `lowk-stt` lost `bench` relative to the
   spec (M0 stub); `FakeEngineFactory` (id `fake`) was added. The session picks
   `settings.stt.engine`, else the first registered engine (hardware choice is M1).
