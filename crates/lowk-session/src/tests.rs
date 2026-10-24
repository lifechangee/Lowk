use super::*;
use lowk_audio::{FakeAudioBackend, FakeAudioRemote};
use lowk_types::fakes::{InlineMainThread, MemSettingsStore, NoopRetriever};
use lowk_types::*;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Rig {
    session: Session,
    store: Arc<MemCallStore>,
    audio: FakeAudioRemote,
}

fn rig(configure: impl FnOnce(&mut SessionDeps)) -> Rig {
    let mut deps = SessionDeps::new(
        Arc::new(InlineMainThread),
        Arc::new(NoopRetriever),
        Arc::new(MemSettingsStore::default()),
    );
    let store = Arc::new(MemCallStore::default());
    let (audio, remote) = FakeAudioBackend::new();
    deps.store = store.clone();
    deps.audio = Arc::new(Mutex::new(audio));
    deps.demo_pace = DemoPace::Instant;
    deps.record_demo = true;
    configure(&mut deps);
    Rig {
        session: Session::new(deps),
        store,
        audio: remote,
    }
}

fn opts(demo: bool) -> StartOpts {
    StartOpts {
        title: None,
        folder: None,
        system: None,
        mic: None,
        demo,
        languages: None,
    }
}

fn phases(rx: &Receiver<AppEvent>) -> Vec<SessionPhase> {
    rx.try_iter()
        .filter_map(|e| match e {
            AppEvent::Session(s) => Some(s.phase),
            _ => None,
        })
        .collect()
}

fn frame(stream: Stream, t: u64) -> Frame16k {
    Frame16k {
        stream,
        t_mono_ns: t,
        pcm: vec![0.0; 320],
    }
}

fn ev(stream: Stream, utt: u64, start_s: u64, end_s: u64, text: &str) -> SttEvent {
    const S: u64 = 1_000_000_000;
    SttEvent {
        stream,
        utt,
        rev: 0,
        kind: SttKind::Final,
        text: text.into(),
        lang: None,
        t_start_ns: start_s * S,
        t_audio_end_ns: end_s * S,
        t_emit_ns: end_s * S,
    }
}

fn brief(typed: Option<&str>) -> AskRequest {
    AskRequest {
        template_id: TEMPLATE_BRIEF.into(),
        typed_text: typed.map(Into::into),
    }
}

#[test]
fn transition_table() {
    use SessionPhase::*;
    let all = [Idle, Starting, Recording, Paused, Stopping];
    let ok: Vec<_> = all
        .iter()
        .flat_map(|a| all.iter().map(move |b| (*a, *b)))
        .filter(|(a, b)| phase_allowed(*a, *b))
        .collect();
    assert_eq!(
        ok,
        vec![
            (Idle, Starting),
            (Starting, Idle),
            (Starting, Recording),
            (Recording, Paused),
            (Recording, Stopping),
            (Paused, Recording),
            (Paused, Stopping),
            (Stopping, Idle),
        ]
    );
}

#[test]
fn live_lifecycle_pause_and_errors() {
    let r = rig(|_| {});
    let rx = r.session.subscribe();
    assert_eq!(r.session.stop().unwrap_err().code, ErrorCode::SessionIdle);
    assert_eq!(r.session.pause().unwrap_err().code, ErrorCode::SessionIdle);

    let id = r.session.start(opts(false)).unwrap();
    let st = r.session.state();
    assert_eq!(
        (st.phase, st.call, st.demo),
        (SessionPhase::Recording, Some(id), false)
    );
    assert_eq!(
        r.session.start(opts(false)).unwrap_err().code,
        ErrorCode::SessionBusy
    );
    assert_eq!(r.session.resume().unwrap_err().code, ErrorCode::SessionBusy);

    // Frames flow while recording and are dropped while paused.
    assert!(r.audio.push_frame(frame(Stream::Me, 1)));
    r.session.pause().unwrap();
    assert_eq!(r.session.pause().unwrap_err().code, ErrorCode::SessionBusy);
    for i in 0..20 {
        r.audio.push_frame(frame(Stream::Me, i));
    }
    r.session.resume().unwrap();
    r.session.stop().unwrap();
    assert_eq!(r.session.state().phase, SessionPhase::Idle);
    assert_eq!(r.audio.stops(), 1);
    assert!(!r.audio.is_capturing());
    assert_eq!(*r.store.finished.lock().unwrap(), 1);

    use SessionPhase::*;
    assert_eq!(
        phases(&rx),
        vec![Starting, Recording, Paused, Recording, Stopping, Idle]
    );
    // Session can be started again.
    r.session.start(opts(false)).unwrap();
    r.session.stop().unwrap();
}

#[test]
fn engine_output_reaches_transcript_bus_and_journal() {
    let r = rig(|_| {});
    let rx = r.session.subscribe();
    r.session.start(opts(false)).unwrap();
    // 25 frames: partials at 10 and 20, final on flush (at stop).
    for i in 0..25 {
        r.audio.push_frame(frame(Stream::System, 1_000 + i));
    }
    let snap = r.session.snapshot();
    assert_eq!(snap.transcript.len(), 1);
    assert!(!snap.transcript[0].is_final);
    r.session.stop().unwrap();

    let snap = r.session.snapshot();
    assert_eq!(snap.transcript.len(), 1);
    assert!(snap.transcript[0].is_final);
    let kinds: Vec<_> = rx
        .try_iter()
        .filter_map(|e| match e {
            AppEvent::Stt(s) => Some((s.rev, s.kind)),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        vec![(0, SttKind::Partial), (1, SttKind::Partial), (2, SttKind::Final)]
    );
    // Only finals are journaled, and the flushed final made it before finish().
    let entries = r.store.entries.lock().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(&entries[0], CallEntry::Final { event } if event.stream == Stream::System));
    let calls = r.store.calls.lock().unwrap();
    assert_eq!(calls[0].speaker_me, "Я");
    assert!(calls[0].started.ends_with('Z'));
}

fn correction_rig(f: impl Fn(&str) -> Option<String> + Send + Sync + 'static, delay_ms: u64) -> Rig {
    rig(|d| {
        d.corrector = Some(Arc::new(lowk_stt::FakeCorrector::new(
            Duration::from_millis(delay_ms),
            f,
        )));
    })
}

#[test]
fn background_correction_replaces_text_and_journals_only_the_better_one() {
    let r = correction_rig(|t| Some(format!("{} (whisper)", t)), 60);
    let rx = r.session.subscribe();
    r.session.start(opts(false)).unwrap();
    for i in 0..25 {
        r.audio.push_frame(frame(Stream::System, 1_000 + i));
    }
    // The live text is on screen at once; Ask sees it without waiting for anything.
    let live = r.session.snapshot().transcript[0].text.clone();
    assert!(live.starts_with("fake utterance"));
    r.session.stop().unwrap();

    let snap = r.session.snapshot();
    assert_eq!(snap.transcript.len(), 1);
    assert!(snap.transcript[0].text.ends_with("(whisper)"));
    let revs: Vec<_> = rx
        .try_iter()
        .filter_map(|e| match e {
            AppEvent::Stt(s) => Some((s.rev, s.kind)),
            _ => None,
        })
        .collect();
    // two partials, the engine final, then the correction as the next revision
    assert_eq!(revs.last(), Some(&(3, SttKind::Final)));
    let entries = r.store.entries.lock().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(&entries[0], CallEntry::Final { event } if event.text.ends_with("(whisper)")));
    let d = r.session.stt_diagnostics().unwrap();
    assert_eq!(d.engine, "fake");
}

#[test]
fn unchanged_or_skipped_correction_keeps_the_live_text_in_the_journal() {
    let r = correction_rig(|_| None, 10);
    r.session.start(opts(false)).unwrap();
    for i in 0..12 {
        r.audio.push_frame(frame(Stream::Me, 1_000 + i));
    }
    r.session.stop().unwrap();
    let entries = r.store.entries.lock().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(&entries[0], CallEntry::Final { event } if event.text.starts_with("fake utterance")));
}

#[test]
fn ask_never_waits_for_correction() {
    // a corrector that takes 3 s must not delay the Ask
    let r = correction_rig(|t| Some(t.to_uppercase()), 3000);
    r.session.start(opts(false)).unwrap();
    for i in 0..12 {
        r.audio.push_frame(frame(Stream::System, 1_000 + i));
    }
    let t0 = std::time::Instant::now();
    r.session.ask(brief(None)).unwrap();
    assert!(t0.elapsed() < Duration::from_millis(500));
    r.session.stop().unwrap();
}

#[test]
fn failed_start_rolls_back_to_idle() {
    let r = rig(|d| {
        let mut s = d.settings.get();
        s.stt.engine = "nope".into();
        d.settings = Arc::new(MemSettingsStore::new(s));
    });
    let rx = r.session.subscribe();
    assert_eq!(
        r.session.start(opts(false)).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(r.session.state().phase, SessionPhase::Idle);
    assert_eq!(phases(&rx), vec![SessionPhase::Starting, SessionPhase::Idle]);
    assert!(!r.audio.is_capturing());
    assert_eq!(*r.store.finished.lock().unwrap(), 1);
}

fn run_demo(r: &Rig) {
    r.session.start(opts(true)).unwrap();
    r.session.wait_demo();
    assert!(r.session.demo_finished());
}

#[test]
fn demo_feed_builds_transcript_and_links_my_answers() {
    let r = rig(|_| {});
    let rx = r.session.subscribe();
    let qa_rx = r.session.subscribe_qa();
    run_demo(&r);
    assert!(r.session.state().demo);

    let snap = r.session.snapshot();
    assert_eq!(snap.transcript.len(), 10);
    assert!(snap.transcript.iter().all(|l| l.is_final));
    assert!(snap.transcript.iter().any(|l| l.text.contains("EBITDA")));
    assert!(snap.transcript.iter().map(|l| l.offset_s).max().unwrap() >= 60);

    let qa = r.session.qa_records();
    assert_eq!(qa.len(), 2);
    assert!(qa[0].question.contains("EBITDA"));
    assert!(qa[0].hint.starts_with("EBITDA"));
    // F14: «Мой ответ» runs from the hint to the user's NEXT Ask (System speech never closes it).
    assert!(qa[0].my_answer.contains("операционная прибыль"));
    assert!(qa[0].my_answer.contains("cash-proxy"));
    assert_eq!(qa[0].closed, Some(CloseReason::NextAsk));
    assert!(qa[1].my_answer.starts_with("Конечно"));
    // The last link stays open until the session ends.
    assert_eq!(qa[1].closed, None);

    // LLM events are well-formed: Start, Deltas, Done per question.
    let llm: Vec<_> = rx
        .try_iter()
        .filter_map(|e| match e {
            AppEvent::Llm(l) => Some(l),
            _ => None,
        })
        .collect();
    let starts = llm.iter().filter(|l| matches!(l, LlmEvent::Start { .. })).count();
    let dones = llm.iter().filter(|l| matches!(l, LlmEvent::Done { .. })).count();
    assert_eq!((starts, dones), (2, 2));
    let streamed: String = llm
        .iter()
        .skip(1)
        .take_while(|l| !matches!(l, LlmEvent::Done { .. }))
        .filter_map(|l| match l {
            LlmEvent::Delta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(streamed, qa[0].hint);
    // QA subscribers saw growing text and the closing update.
    let ups: Vec<_> = qa_rx.try_iter().collect();
    assert!(ups.iter().any(|u| u.closed == Some(CloseReason::NextAsk)));
    assert!(ups
        .iter()
        .any(|u| u.closed.is_none() && u.text.contains("EBITDA")));

    r.session.stop().unwrap();
    let entries = r.store.entries.lock().unwrap();
    let count = |f: fn(&CallEntry) -> bool| entries.iter().filter(|e| f(e)).count();
    assert_eq!(count(|e| matches!(e, CallEntry::Final { .. })), 10);
    assert_eq!(count(|e| matches!(e, CallEntry::Qa { .. })), 2);
    assert_eq!(count(|e| matches!(e, CallEntry::MyAnswer { .. })), 2);
    assert!(*r.store.materializations.lock().unwrap() >= 2);
    assert_eq!(r.session.state().phase, SessionPhase::Idle);
    assert!(!r.session.state().demo);
}

#[test]
fn demo_feed_is_deterministic_across_runs() {
    let digest = || {
        let r = rig(|_| {});
        run_demo(&r);
        let t = r.session.snapshot().transcript;
        let q: Vec<_> = r
            .session
            .qa_records()
            .into_iter()
            .map(|q| (q.question, q.hint, q.my_answer, q.closed))
            .collect();
        r.session.stop().unwrap();
        (t, q)
    };
    assert_eq!(digest(), digest());
}

#[test]
fn demo_without_record_flag_skips_store() {
    let r = rig(|d| d.record_demo = false);
    run_demo(&r);
    r.session.stop().unwrap();
    assert!(r.store.calls.lock().unwrap().is_empty());
}

#[test]
fn stop_cancels_a_realtime_demo() {
    let r = rig(|d| d.demo_pace = DemoPace::Realtime(1.0));
    r.session.start(opts(true)).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    r.session.stop().unwrap();
    assert!(!r.session.demo_finished());
    assert_eq!(r.session.state().phase, SessionPhase::Idle);
}

#[test]
fn live_ask_streams_hint_and_collects_my_answer() {
    let r = rig(|d| d.hints = Arc::new(StubHints("Короткий ответ: да.".into())));
    let rx = r.session.subscribe();
    assert_eq!(
        r.session.ask(brief(None)).unwrap_err().code,
        ErrorCode::SessionIdle
    );
    r.session.start(opts(false)).unwrap();
    let bad = AskRequest {
        template_id: "zzz".into(),
        typed_text: None,
    };
    assert_eq!(r.session.ask(bad).unwrap_err().code, ErrorCode::NotFound);
    // Question comes from the latest System final when no typed text is given.
    r.session
        .inject_stt(ev(Stream::System, 1, 0, 2, "Что такое EBITDA?"));
    let qa_id = r.session.ask(brief(None)).unwrap();
    loop {
        match rx.recv_timeout(Duration::from_secs(5)).expect("llm done") {
            AppEvent::Llm(LlmEvent::Done { .. }) => break,
            AppEvent::Llm(LlmEvent::Error { error, .. }) => panic!("{error:?}"),
            _ => {}
        }
    }
    let rec = r.session.qa_records().remove(0);
    assert_eq!((rec.qa_id, rec.question.as_str()), (qa_id, "Что такое EBITDA?"));
    assert_eq!(rec.hint, "Короткий ответ: да.");
    // The user answers after the hint (event time is "now" + a bit).
    let t = now_mono_ns() / 1_000_000_000 + 2;
    r.session
        .inject_stt(ev(Stream::Me, 1, t, t + 2, "Это прибыль до налогов."));
    assert_eq!(
        r.session.my_answer(&qa_id).as_deref(),
        Some("Это прибыль до налогов.")
    );
    r.session.stop().unwrap();
    let entries = r.store.entries.lock().unwrap();
    assert!(entries
        .iter()
        .any(|e| matches!(e, CallEntry::Qa { question, .. } if question == "Что такое EBITDA?")));
    assert!(entries
        .iter()
        .any(|e| matches!(e, CallEntry::MyAnswer { text, .. } if text == "Это прибыль до налогов.")));
}

struct FailingHints;
impl HintGenerator for FailingHints {
    fn generate(&self, _r: &HintRequest, _d: &mut dyn FnMut(&str)) -> Result<String> {
        Err(AppError::new(ErrorCode::LlmProvider))
    }
}

#[test]
fn failed_ask_emits_error_and_arms_nothing() {
    let r = rig(|d| d.hints = Arc::new(FailingHints));
    let rx = r.session.subscribe();
    r.session.start(opts(false)).unwrap();
    r.session.ask(brief(Some("q"))).unwrap();
    loop {
        match rx.recv_timeout(Duration::from_secs(5)).expect("llm error") {
            AppEvent::Llm(LlmEvent::Error { error, .. }) => {
                assert_eq!(error.code, ErrorCode::LlmProvider);
                break;
            }
            AppEvent::Llm(LlmEvent::Done { .. }) => panic!("unexpected done"),
            _ => {}
        }
    }
    r.session.stop().unwrap();
    assert!(r.session.qa_records().is_empty());
}

#[test]
fn ask_takes_the_question_from_the_utterance_still_being_spoken() {
    // M1 QA: the owner presses Ask while the interviewer is still mid-question. The question
    // must be that partial, not the previous (final) sentence.
    let r = rig(|d| d.hints = Arc::new(StubHints("ok".into())));
    r.session.start(opts(false)).unwrap();
    r.session
        .inject_stt(ev(Stream::System, 1, 0, 2, "Расскажите о себе."));
    let mut p = ev(Stream::System, 2, 3, 4, "А что такое EBITDA");
    p.kind = SttKind::Partial;
    r.session.inject_stt(p);
    r.session.ask(brief(None)).unwrap();
    let rec = r.session.qa_records().remove(0);
    assert_eq!(rec.question, "А что такое EBITDA");
    r.session.stop().unwrap();
}

/// Streams for a while, so the test can speak while the hint is still being generated.
struct SlowHints;
impl HintGenerator for SlowHints {
    fn generate(&self, _r: &HintRequest, d: &mut dyn FnMut(&str)) -> Result<String> {
        d("раз ");
        std::thread::sleep(Duration::from_millis(600));
        d("два");
        Ok("раз два".into())
    }
}

#[test]
fn my_answer_collects_speech_spoken_while_the_hint_is_still_streaming() {
    // M1 QA: the owner answers along with the generation. Speech that ends before the hint
    // finishes must not be lost.
    let r = rig(|d| d.hints = Arc::new(SlowHints));
    r.session.start(opts(false)).unwrap();
    let qa_id = r.session.ask(brief(Some("Что такое EBITDA?"))).unwrap();
    let t = now_mono_ns() / 1_000_000_000 + 1;
    r.session
        .inject_stt(ev(Stream::Me, 1, t, t + 1, "Это операционная прибыль"));
    assert_eq!(
        r.session.my_answer(&qa_id).as_deref(),
        Some("Это операционная прибыль"),
        "collected before the hint is done"
    );
    r.session.stop().unwrap();
    let entries = r.store.entries.lock().unwrap();
    assert!(entries
        .iter()
        .any(|e| matches!(e, CallEntry::MyAnswer { text, .. } if text == "Это операционная прибыль")));
    assert!(entries
        .iter()
        .any(|e| matches!(e, CallEntry::Qa { answer, .. } if answer == "раз два")));
}

#[test]
fn s4_echo_is_not_transcribed_journaled_or_collected_as_my_answer() {
    let r = rig(|d| d.hints = Arc::new(StubHints("Подсказка.".into())));
    let rx = r.session.subscribe();
    r.session.start(opts(false)).unwrap();
    let t = now_mono_ns() / 1_000_000_000;
    r.session.inject_engine_stt(ev(
        Stream::System,
        1,
        t,
        t + 4,
        "Расскажи коротко, чем занимался последние два года?",
    ));
    let qa_id = r.session.ask(brief(None)).unwrap();
    loop {
        if let AppEvent::Llm(LlmEvent::Done { .. }) = rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            break;
        }
    }
    // the speakers' echo of the question, transcribed a little differently, arrives as «Я»
    let tt = now_mono_ns() / 1_000_000_000 + 2;
    r.session.inject_engine_stt(ev(
        Stream::Me,
        1,
        tt,
        tt + 4,
        "Расскажи коротко чем занимался последние два года.",
    ));
    // ...then the user really answers
    r.session.inject_engine_stt(ev(
        Stream::Me,
        2,
        tt + 6,
        tt + 9,
        "Я работал продуктовым менеджером.",
    ));
    assert_eq!(
        r.session.my_answer(&qa_id).as_deref(),
        Some("Я работал продуктовым менеджером.")
    );
    r.session.stop().unwrap();
    let entries = r.store.entries.lock().unwrap();
    let me_finals: Vec<_> = entries
        .iter()
        .filter_map(|e| match e {
            CallEntry::Final { event } if event.stream == Stream::Me => Some(event.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(me_finals, vec!["Я работал продуктовым менеджером."]);
}

#[test]
fn s5_target_lost_and_restored_reach_the_bus_as_notices() {
    let r = rig(|_| {});
    let rx = r.session.subscribe();
    r.session.start(opts(false)).unwrap();
    assert!(r.audio.push_event(lowk_audio::AudioEvent::TargetLost));
    assert!(r.audio.push_event(lowk_audio::AudioEvent::TargetRestored));
    let got: Vec<AudioNotice> = rx
        .try_iter()
        .filter_map(|e| match e {
            AppEvent::AudioNotice(n) => Some(n),
            _ => None,
        })
        .collect();
    assert_eq!(got, vec![AudioNotice::TargetLost, AudioNotice::TargetRestored]);
    // the call goes on
    assert_eq!(r.session.state().phase, SessionPhase::Recording);
    r.session.stop().unwrap();
}

/// Per-call languages: the call stores the picked set limited to what the engine supports
/// (the fake engine decodes ru + en); no pick = the default set of the settings.
#[test]
fn call_languages_follow_the_pick_and_the_engine() {
    let start_with = |langs: Option<Vec<&str>>| {
        let r = rig(|_| {});
        let mut o = opts(false);
        o.languages = langs.map(|v| v.into_iter().map(String::from).collect());
        r.session.start(o).unwrap();
        let l = r.store.calls.lock().unwrap()[0].languages.clone();
        r.session.stop().unwrap();
        l
    };
    assert_eq!(start_with(None), vec!["ru", "en"]);
    assert_eq!(start_with(Some(vec!["en"])), vec!["en"]);
    assert_eq!(start_with(Some(vec!["ru"])), vec!["ru"]);
    // "pl" is not decodable by the fake engine: dropped; nothing left -> the default set
    assert_eq!(start_with(Some(vec!["pl", "en"])), vec!["en"]);
    assert_eq!(start_with(Some(vec!["pl"])), vec!["ru", "en"]);
    assert_eq!(start_with(Some(vec!["EN-us", "en"])), vec!["en"]);
}

/// The last pick in the settings (`stt.last_languages`) is what a call without an explicit pick
/// (record bind) uses; the default only applies while nothing was picked.
#[test]
fn remembered_languages_apply_without_an_explicit_pick() {
    let mut s = Settings::default();
    s.stt.last_languages = vec!["en".into()];
    let r = rig(|d| d.settings = Arc::new(MemSettingsStore::new(s)));
    r.session.start(opts(false)).unwrap();
    assert_eq!(r.store.calls.lock().unwrap()[0].languages, vec!["en"]);
    r.session.stop().unwrap();
}

/// Records what the session asked the retriever and what the generator was given.
#[derive(Default)]
struct SpyRetriever {
    queries: Mutex<Vec<RetrievalQuery>>,
}
impl Retriever for SpyRetriever {
    fn retrieve(&self, q: &RetrievalQuery) -> Result<Vec<ContextChunk>> {
        self.queries.lock().unwrap().push(q.clone());
        lowk_types::fakes::FakeRetriever.retrieve(q)
    }
    fn retrieve_pack(&self, q: &RetrievalQuery) -> Result<ContextPack> {
        self.queries.lock().unwrap().push(q.clone());
        lowk_types::fakes::FakeRetriever.retrieve_pack(q)
    }
}
struct SpyHandle(Arc<SpyRetriever>);
impl Retriever for SpyHandle {
    fn retrieve(&self, q: &RetrievalQuery) -> Result<Vec<ContextChunk>> {
        self.0.retrieve(q)
    }
    fn retrieve_pack(&self, q: &RetrievalQuery) -> Result<ContextPack> {
        self.0.retrieve_pack(q)
    }
}

struct SeenHints {
    seen: Mutex<Vec<HintRequest>>,
    /// `Some(code)`: preflight refuses (local-only without a local model, no key).
    refuse: Option<ErrorCode>,
    cloud: bool,
}
impl HintGenerator for SeenHints {
    fn generate(&self, _r: &HintRequest, _d: &mut dyn FnMut(&str)) -> Result<String> {
        unreachable!("the session uses generate_ex")
    }
    fn preflight(&self, _f: &FolderRef) -> Result<bool> {
        match self.refuse {
            Some(c) => Err(AppError::new(c)),
            None => Ok(self.cloud),
        }
    }
    fn generate_ex(
        &self,
        req: &HintRequest,
        on_sources: &mut dyn FnMut(Vec<SourceRef>, u32),
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<HintOutcome> {
        self.seen.lock().unwrap().push(req.clone());
        let mut ids = vec![];
        let mut srcs = vec![];
        for c in req
            .pack
            .instructions
            .iter()
            .chain(&req.pack.pinned)
            .chain(&req.pack.relevant)
        {
            ids.push(c.id.clone());
            srcs.push(c.source_ref.clone());
        }
        on_sources(srcs, req.pack.withheld);
        on_delta("ответ");
        Ok(HintOutcome {
            answer: "ответ".into(),
            sent_chunk_ids: ids,
            ..HintOutcome::default()
        })
    }
}

fn user_folder_opts() -> StartOpts {
    StartOpts {
        folder: Some(FolderRef::User(Ulid::from_parts(5, 5))),
        ..opts(false)
    }
}

fn wait_llm_end(rx: &Receiver<AppEvent>) -> Vec<LlmEvent> {
    let mut seen = vec![];
    loop {
        if let AppEvent::Llm(e) = rx.recv_timeout(Duration::from_secs(5)).expect("llm end") {
            let end = matches!(e, LlmEvent::Done { .. } | LlmEvent::Error { .. });
            seen.push(e);
            if end {
                return seen;
            }
        }
    }
}

#[test]
fn ask_uses_the_retrieval_pack_and_emits_sources_before_the_first_delta() {
    let spy = Arc::new(SpyRetriever::default());
    let hints = Arc::new(SeenHints {
        seen: Mutex::new(vec![]),
        refuse: None,
        cloud: true,
    });
    let (s2, h2) = (spy.clone(), hints.clone());
    let r = rig(move |d| {
        d.retriever = Arc::new(SpyHandle(s2));
        d.hints = h2;
    });
    let rx = r.session.subscribe();
    r.session.start(user_folder_opts()).unwrap();
    r.session.ask(brief(Some("Что такое EBITDA?"))).unwrap();
    let evs = wait_llm_end(&rx);
    let kinds: Vec<&str> = evs
        .iter()
        .map(|e| match e {
            LlmEvent::Start { .. } => "start",
            LlmEvent::Sources { .. } => "sources",
            LlmEvent::Meta { .. } => "meta",
            LlmEvent::Delta { .. } => "delta",
            LlmEvent::Done { .. } => "done",
            LlmEvent::Error { .. } => "error",
        })
        .collect();
    assert_eq!(kinds, ["start", "meta", "sources", "delta", "done"]);
    let LlmEvent::Sources { sources, .. } = &evs[2] else {
        unreachable!()
    };
    assert_eq!(sources.len(), 3, "instructions, pinned doc, relevant doc");
    // first turn: folder context, cloud flag, folder scope, previous calls
    let q1 = spy.queries.lock().unwrap()[0].clone();
    assert!(q1.include_folder_context && q1.include_previous_calls && q1.cloud_provider);
    assert_eq!(q1.scope.folder, Some(Ulid::from_parts(5, 5)));
    assert_eq!(q1.scope.call, r.session.state().call);
    assert!(q1.exclude.is_empty());
    // second turn: no folder context again, the first turn's chunks are excluded
    r.session.ask(brief(Some("А ARR?"))).unwrap();
    wait_llm_end(&rx);
    let q2 = spy.queries.lock().unwrap()[1].clone();
    assert!(!q2.include_folder_context);
    assert_eq!(q2.exclude.len(), 3);
    assert!(!hints.seen.lock().unwrap()[1].first_turn);
    r.session.stop().unwrap();
}

#[test]
fn refused_ask_retrieves_and_sends_nothing() {
    let spy = Arc::new(SpyRetriever::default());
    let hints = Arc::new(SeenHints {
        seen: Mutex::new(vec![]),
        refuse: Some(ErrorCode::LocalOnlyBlocked),
        cloud: true,
    });
    let (s2, h2) = (spy.clone(), hints.clone());
    let r = rig(move |d| {
        d.retriever = Arc::new(SpyHandle(s2));
        d.hints = h2;
    });
    let rx = r.session.subscribe();
    r.session.start(user_folder_opts()).unwrap();
    r.session.ask(brief(Some("секрет?"))).unwrap();
    let evs = wait_llm_end(&rx);
    assert!(matches!(&evs[1], LlmEvent::Error { error, .. } if error.code == ErrorCode::LocalOnlyBlocked));
    assert!(
        spy.queries.lock().unwrap().is_empty(),
        "no retrieval for a refused ask"
    );
    assert!(hints.seen.lock().unwrap().is_empty(), "the generator never ran");
    assert!(r.session.qa_records().is_empty());
    r.session.stop().unwrap();
}

fn marker_texts(r: &Rig) -> Vec<String> {
    r.store
        .entries
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            CallEntry::Marker { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn finals_of(r: &Rig, stream: Stream) -> usize {
    r.store
        .entries
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, CallEntry::Final { event } if event.stream == stream))
        .count()
}

#[test]
fn mic_off_drops_me_frames_keeps_system_and_writes_markers() {
    let r = rig(|_| {});
    let rx = r.session.subscribe();
    r.session.start(opts(false)).unwrap();
    assert!(r.session.state().mic_on, "default: the microphone records");
    // mic on: both streams reach the engines (25 frames = partials + a final on flush)
    for i in 0..25 {
        r.audio.push_frame(frame(Stream::Me, 1_000 + i));
    }
    r.session.set_mic_enabled(false).unwrap();
    assert!(!r.session.state().mic_on);
    let transcript_len = r.session.snapshot().transcript.len();
    // mic off: 40 Me frames go nowhere, System frames still transcribe
    for i in 0..40 {
        r.audio.push_frame(frame(Stream::Me, 2_000 + i));
    }
    assert_eq!(
        r.session.snapshot().transcript.len(),
        transcript_len,
        "no new Me line while muted"
    );
    for i in 0..25 {
        r.audio.push_frame(frame(Stream::System, 3_000 + i));
    }
    assert!(r.audio.is_capturing(), "system-audio capture is not torn down");
    assert_eq!(
        r.session.snapshot().transcript.len(),
        transcript_len + 1,
        "System keeps transcribing"
    );
    // idempotent: a repeated off adds no second marker
    r.session.set_mic_enabled(false).unwrap();
    r.session.set_mic_enabled(true).unwrap();
    assert!(r.session.state().mic_on);
    r.session.stop().unwrap();

    assert_eq!(marker_texts(&r), vec!["Микрофон выключен", "Микрофон включён"]);
    assert_eq!(
        finals_of(&r, Stream::Me),
        1,
        "only the speech before the mute is journaled"
    );
    assert_eq!(finals_of(&r, Stream::System), 1);
    let states: Vec<bool> = rx
        .try_iter()
        .filter_map(|e| match e {
            AppEvent::Session(s) => Some(s.mic_on),
            _ => None,
        })
        .collect();
    assert!(states.contains(&false) && states.last() == Some(&true));
}

#[test]
fn mic_marker_follows_the_ui_language() {
    let r = rig(|d| {
        let mut s = Settings::default();
        s.ui.language = UiLanguage::En;
        d.settings = Arc::new(MemSettingsStore::new(s));
    });
    r.session.start(opts(false)).unwrap();
    r.session.set_mic_enabled(false).unwrap();
    r.session.set_mic_enabled(true).unwrap();
    r.session.stop().unwrap();
    assert_eq!(marker_texts(&r), vec!["Microphone off", "Microphone on"]);
}

#[test]
fn mic_toggle_outside_a_recording_is_refused() {
    let r = rig(|_| {});
    assert_eq!(
        r.session.set_mic_enabled(false).unwrap_err().code,
        ErrorCode::SessionIdle
    );
}

#[test]
fn new_call_mic_follows_folder_preset_and_global_default() {
    let folder = FolderRef::User(Ulid::from_parts(7, 7));
    // (preset of the folder, global record_mic) -> mic on at start
    let cases = [
        (MicPreset::Inherit, true, true),
        (MicPreset::Inherit, false, false),
        (MicPreset::On, false, true),
        (MicPreset::Off, true, false),
    ];
    for (preset, global, want) in cases {
        let r = rig(|d| {
            d.folder_mic = Arc::new(move |_| preset);
            let mut s = Settings::default();
            s.audio.record_mic = global;
            d.settings = Arc::new(MemSettingsStore::new(s));
        });
        r.session
            .start(StartOpts {
                folder: Some(folder.clone()),
                ..opts(false)
            })
            .unwrap();
        assert_eq!(r.session.state().mic_on, want, "{preset:?} / global {global}");
        // a muted start really drops the Me frames
        for i in 0..25 {
            r.audio.push_frame(frame(Stream::Me, 1_000 + i));
        }
        assert_eq!(
            r.session.snapshot().transcript.len(),
            usize::from(want),
            "{preset:?} / global {global}"
        );
        r.session.stop().unwrap();
        assert!(
            marker_texts(&r).is_empty(),
            "the preset start is not a toggle: no marker"
        );
    }
}

#[test]
fn call_starting_with_mic_off_never_opens_the_mic_source() {
    let r = rig(|d| {
        d.folder_mic = Arc::new(|_| MicPreset::Off);
    });
    r.session.start(opts(false)).unwrap();
    assert!(r.audio.is_capturing(), "the system stream is started");
    assert_eq!(r.audio.mic_opens(), 0, "mic source never opened");
    assert!(!r.audio.mic_open());
    // global setting off behaves the same
    r.session.stop().unwrap();
    assert_eq!((r.audio.mic_opens(), r.audio.mic_closes()), (0, 0));
}

#[test]
fn global_mic_off_never_opens_the_mic_source() {
    let r = rig(|d| {
        let mut s = Settings::default();
        s.audio.record_mic = false;
        d.settings = Arc::new(MemSettingsStore::new(s));
    });
    r.session.start(opts(false)).unwrap();
    assert_eq!(r.audio.mic_opens(), 0);
    r.session.stop().unwrap();
    assert_eq!(r.audio.mic_opens(), 0);
}

#[test]
fn toggle_off_closes_the_source_and_toggle_on_reopens_it() {
    let r = rig(|_| {});
    r.session.start(opts(false)).unwrap();
    assert_eq!(
        (r.audio.mic_open(), r.audio.mic_opens(), r.audio.mic_closes()),
        (true, 1, 0)
    );
    r.session.set_mic_enabled(false).unwrap();
    assert_eq!(
        (r.audio.mic_open(), r.audio.mic_opens(), r.audio.mic_closes()),
        (false, 1, 1)
    );
    // repeated off does nothing
    r.session.set_mic_enabled(false).unwrap();
    assert_eq!(r.audio.mic_closes(), 1);
    r.session.set_mic_enabled(true).unwrap();
    assert_eq!(
        (r.audio.mic_open(), r.audio.mic_opens(), r.audio.mic_closes()),
        (true, 2, 1)
    );
    // Me speech after the reopen is transcribed again
    for i in 0..25 {
        r.audio.push_frame(frame(Stream::Me, 5_000 + i));
    }
    r.session.stop().unwrap();
    assert_eq!(finals_of(&r, Stream::Me), 1);
    assert!(!r.audio.mic_open(), "stop releases the source");
    assert_eq!(r.audio.mic_closes(), 2);
}

#[test]
fn mic_started_off_can_be_switched_on_later() {
    let r = rig(|d| {
        d.folder_mic = Arc::new(|_| MicPreset::Off);
    });
    r.session.start(opts(false)).unwrap();
    r.session.set_mic_enabled(true).unwrap();
    assert_eq!((r.audio.mic_open(), r.audio.mic_opens()), (true, 1));
    r.session.stop().unwrap();
    assert!(!r.audio.mic_open());
}

#[test]
fn system_frames_stay_continuous_across_mic_toggles_and_me_is_flushed() {
    let r = rig(|_| {});
    r.session.start(opts(false)).unwrap();
    // half-spoken Me utterance, then mic off: it ends with a clean final instead of hanging
    for i in 0..10 {
        r.audio.push_frame(frame(Stream::Me, 1_000 + i));
    }
    let mut sys_pushed = 0;
    for round in 0..3u64 {
        for i in 0..5 {
            assert!(r
                .audio
                .push_frame(frame(Stream::System, 10_000 + round * 100 + i)));
            sys_pushed += 1;
        }
        r.session.set_mic_enabled(round % 2 == 1).unwrap();
        for i in 0..5 {
            assert!(r
                .audio
                .push_frame(frame(Stream::System, 10_050 + round * 100 + i)));
            sys_pushed += 1;
        }
    }
    assert!(
        r.audio.is_capturing(),
        "no capture restart: one start, system stream uninterrupted"
    );
    assert_eq!(r.audio.starts(), 1);
    assert_eq!(sys_pushed, 30);
    r.session.stop().unwrap();
    assert!(
        finals_of(&r, Stream::Me) >= 1,
        "the Me utterance was flushed at toggle-off"
    );
}

#[test]
fn keep_audio_taps_exactly_the_frames_the_engines_get() {
    for keep in [false, true] {
        let r = rig(|d| {
            let mut s = Settings::default();
            s.audio.keep_audio = keep;
            d.settings = Arc::new(MemSettingsStore::new(s));
        });
        r.session.start(opts(false)).unwrap();
        r.audio.push_frame(frame(Stream::Me, 1_000));
        r.audio.push_frame(frame(Stream::System, 2_000));
        // a muted microphone and a pause leave holes (silence in the saved file), not audio
        r.session.set_mic_enabled(false).unwrap();
        r.audio.push_frame(frame(Stream::Me, 3_000));
        r.audio.push_frame(frame(Stream::System, 4_000));
        r.session.pause().unwrap();
        r.audio.push_frame(frame(Stream::System, 5_000));
        r.session.stop().unwrap();
        let kept = r.store.kept_frames.lock().unwrap().clone();
        if keep {
            assert_eq!(
                kept,
                [
                    (Stream::Me, 1_000),
                    (Stream::System, 2_000),
                    (Stream::System, 4_000)
                ]
            );
        } else {
            assert!(kept.is_empty(), "audio is only kept when the setting is on");
        }
    }
}

// ---- screenshots ----------------------------------------------------------------------------

#[derive(Default)]
struct FakeShots {
    q: Mutex<Vec<QueuedImage>>,
    grab: Mutex<Vec<QueuedImage>>,
}
fn img(id: u64) -> QueuedImage {
    QueuedImage {
        id,
        index: 0,
        primary: true,
        width: 4,
        height: 4,
        png: Arc::new(vec![1, 2, 3]),
    }
}
impl ShotSource for FakeShots {
    fn peek(&self) -> Vec<QueuedImage> {
        self.q.lock().unwrap().clone()
    }
    fn capture_if_empty(&self) -> Vec<QueuedImage> {
        self.grab.lock().unwrap().clone()
    }
    fn consume(&self, ids: &[u64]) {
        self.q.lock().unwrap().retain(|i| !ids.contains(&i.id));
    }
    fn clear(&self) {
        self.q.lock().unwrap().clear();
    }
}

/// Echoes the images it was given as "sent"; `fail` makes the generator error after seeing them.
struct ImgHints {
    seen: Mutex<Vec<(usize, bool)>>,
    fail: bool,
}
impl HintGenerator for ImgHints {
    fn generate(&self, _r: &HintRequest, _d: &mut dyn FnMut(&str)) -> Result<String> {
        unreachable!()
    }
    fn generate_ex(
        &self,
        req: &HintRequest,
        on_sources: &mut dyn FnMut(Vec<SourceRef>, u32),
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<HintOutcome> {
        self.seen
            .lock()
            .unwrap()
            .push((req.images.len(), req.vision_template));
        if self.fail {
            return Err(AppError::new(ErrorCode::ImageTooLarge));
        }
        on_sources(vec![], 0);
        on_delta("ответ");
        Ok(HintOutcome {
            answer: "ответ".into(),
            images: req
                .images
                .iter()
                .map(|i| SentImage {
                    id: i.id,
                    ext: "jpg",
                    bytes: Arc::new(vec![9; 10]),
                    width: 4,
                    height: 4,
                })
                .collect(),
            ..HintOutcome::default()
        })
    }
}

fn shot_rig(fail: bool) -> (Rig, Arc<FakeShots>, Arc<ImgHints>) {
    let shots = Arc::new(FakeShots::default());
    let hints = Arc::new(ImgHints {
        seen: Mutex::new(vec![]),
        fail,
    });
    shots.q.lock().unwrap().extend([img(1), img(2)]);
    let (s2, h2) = (shots.clone(), hints.clone());
    let r = rig(move |d| {
        d.shots = s2;
        d.hints = h2;
    });
    (r, shots, hints)
}

#[test]
fn ask_sends_the_queue_clears_it_on_success_and_records_the_images() {
    let (r, shots, hints) = shot_rig(false);
    let rx = r.session.subscribe();
    r.session.start(opts(false)).unwrap();
    r.session.ask(brief(Some("что на слайде?"))).unwrap();
    let evs = wait_llm_end(&rx);
    assert!(
        evs.iter().any(|e| matches!(e, LlmEvent::Meta { images: 2, .. })),
        "{evs:?}"
    );
    assert_eq!(hints.seen.lock().unwrap()[0].0, 2);
    assert!(
        shots.q.lock().unwrap().is_empty(),
        "queue cleared after a successful send"
    );
    let entries = r.store.entries.lock().unwrap().clone();
    let imgs: Vec<_> = entries
        .iter()
        .filter_map(|e| match e {
            CallEntry::Image {
                rel_path,
                source,
                caption,
                ..
            } => Some((rel_path.clone(), source.clone(), caption.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(imgs.len(), 2);
    let qa_id = entries
        .iter()
        .find_map(|e| match e {
            CallEntry::Qa { id, .. } => Some(id.to_string()),
            _ => None,
        })
        .unwrap();
    assert!(imgs.iter().all(|(p, s, c)| p.starts_with("mem.assets/")
        && *s == format!("ask:{qa_id}")
        && c == "что на слайде?"));
    assert_eq!(r.store.images.lock().unwrap().len(), 2);
    r.session.stop().unwrap();
}

#[test]
fn failed_ask_keeps_the_queue_and_records_nothing() {
    let (r, shots, _h) = shot_rig(true);
    let rx = r.session.subscribe();
    r.session.start(opts(false)).unwrap();
    r.session.ask(brief(Some("?"))).unwrap();
    let evs = wait_llm_end(&rx);
    assert!(
        matches!(evs.last(), Some(LlmEvent::Error { error, .. }) if error.code == ErrorCode::ImageTooLarge)
    );
    assert_eq!(shots.q.lock().unwrap().len(), 2, "queue kept after a failure");
    assert!(!r
        .store
        .entries
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, CallEntry::Image { .. })));
    r.session.stop().unwrap();
}

#[test]
fn stopping_the_session_empties_the_queue() {
    let (r, shots, _h) = shot_rig(false);
    r.session.start(opts(false)).unwrap();
    assert_eq!(shots.q.lock().unwrap().len(), 2);
    r.session.stop().unwrap();
    assert!(
        shots.q.lock().unwrap().is_empty(),
        "nothing leaks into the next call"
    );
}

#[test]
fn vision_template_with_an_empty_queue_grabs_the_screen() {
    let shots = Arc::new(FakeShots::default());
    shots.grab.lock().unwrap().push(img(7));
    let hints = Arc::new(ImgHints {
        seen: Mutex::new(vec![]),
        fail: false,
    });
    let mut st = Settings::default();
    st.templates.push(Template {
        id: "look".into(),
        names: LocalizedName {
            ru: "Смотри".into(),
            en: "Look".into(),
        },
        instruction: "Look".into(),
        needs_my_answer: false,
        vision: true,
        reasoning: false,
    });
    let (s2, h2) = (shots.clone(), hints.clone());
    let r = rig(move |d| {
        d.shots = s2;
        d.hints = h2;
        d.settings = Arc::new(MemSettingsStore::new(st));
    });
    let rx = r.session.subscribe();
    r.session.start(opts(false)).unwrap();
    // a plain template with an empty queue sends no image; the vision one grabs the screen
    r.session.ask(brief(Some("a"))).unwrap();
    wait_llm_end(&rx);
    r.session
        .ask(AskRequest {
            template_id: "look".into(),
            typed_text: Some("b".into()),
        })
        .unwrap();
    wait_llm_end(&rx);
    assert_eq!(*hints.seen.lock().unwrap(), vec![(0, false), (1, true)]);
    r.session.stop().unwrap();
}
