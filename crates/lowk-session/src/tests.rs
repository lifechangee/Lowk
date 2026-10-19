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
    // «Мой ответ» = both Me finals after the hint, up to the next long System line.
    assert!(qa[0].my_answer.contains("операционная прибыль"));
    assert!(qa[0].my_answer.contains("cash-proxy"));
    assert!(!qa[0].my_answer.contains("Конечно"));
    assert_eq!(qa[0].closed, Some(CloseReason::SystemQuestion));
    assert!(qa[1].my_answer.starts_with("Конечно"));
    assert!(!qa[1].my_answer.contains("Спасибо"));
    assert_eq!(qa[1].closed, Some(CloseReason::SystemQuestion));

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
    assert!(ups.iter().any(|u| u.closed == Some(CloseReason::SystemQuestion)));
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
