//! The `Session` orchestrator and its state machine
//! (Idle -> Starting -> Recording <-> Paused -> Stopping -> Idle).
use crate::bus::EventBus;
use crate::deps::{CallRecorder, HintRequest, SessionDeps};
use crate::qa::{ClosedAnswer, QaOutput, QaRecord, QaTracker, QaUpdate};
use crate::replay::{rebase, Pacer, ReplayItem, ReplayKind, ReplaySource};
use crate::transcript::{Applied, TranscriptBuf};
use lowk_audio::{AudioEvent, CaptureConfig, CaptureHandle, FrameSink};
use lowk_stt::{SttConfig, SttEngine};
use lowk_types::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;

const NS_PER_MS: u64 = 1_000_000;
/// Journal -> `.md` cadence (call time).
const MATERIALIZE_EVERY_NS: u64 = 20_000_000_000;

/// Allowed phase transitions.
pub fn phase_allowed(from: SessionPhase, to: SessionPhase) -> bool {
    use SessionPhase::*;
    matches!(
        (from, to),
        (Idle, Starting)
            | (Starting, Recording)
            | (Starting, Idle)
            | (Recording, Paused)
            | (Paused, Recording)
            | (Recording, Stopping)
            | (Paused, Stopping)
            | (Stopping, Idle)
    )
}

type SharedEngine = Arc<Mutex<Box<dyn SttEngine>>>;
type Engines = Arc<HashMap<Stream, SharedEngine>>;

struct Core {
    state: SessionState,
    clock: Option<SessionClock>,
    transcript: TranscriptBuf,
    qa: QaTracker,
    recorder: Option<Box<dyn CallRecorder>>,
    capture: Option<Box<dyn CaptureHandle>>,
    engines: Option<Engines>,
    threads: Vec<JoinHandle<()>>,
    demo_thread: Option<JoinHandle<()>>,
    /// First transcript seq not yet sent to the LLM.
    llm_cursor: u64,
    /// Start of the transcript range the next Q&A covers (call-relative ns base).
    range_from: MonoNs,
    last_materialize: MonoNs,
    pending_q: HashMap<QaId, String>,
    last_meta: Option<CallMeta>,
    stt_errors: Vec<SttError>,
}

impl Core {
    fn new() -> Self {
        Self {
            state: SessionState {
                phase: SessionPhase::Idle,
                call: None,
                demo: false,
            },
            clock: None,
            transcript: TranscriptBuf::default(),
            qa: QaTracker::default(),
            recorder: None,
            capture: None,
            engines: None,
            threads: vec![],
            demo_thread: None,
            llm_cursor: 0,
            range_from: 0,
            last_materialize: 0,
            pending_q: HashMap::new(),
            last_meta: None,
            stt_errors: vec![],
        }
    }

    fn set_phase(&mut self, to: SessionPhase, out: &mut Out) {
        debug_assert!(
            phase_allowed(self.state.phase, to),
            "illegal transition {:?} -> {:?}",
            self.state.phase,
            to
        );
        self.state.phase = to;
        out.events.push(AppEvent::Session(self.state.clone()));
    }

    fn push_entry(&mut self, e: CallEntry) {
        if let Some(r) = self.recorder.as_mut() {
            if let Err(err) = r.push(e) {
                tracing::warn!(?err, "call journal write failed");
            }
        }
    }

    /// Journals closed answers and returns the updates to publish.
    fn absorb_qa(&mut self, o: QaOutput, out: &mut Out) {
        let clock = self.clock;
        for ClosedAnswer {
            qa_id,
            text,
            first_at,
            ..
        } in o.closed
        {
            let at_s = match (clock, first_at) {
                (Some(c), Some(t)) => (c.offset_ns(t) / 1_000_000_000) as u32,
                _ => 0,
            };
            self.push_entry(CallEntry::MyAnswer { qa_id, at_s, text });
        }
        out.qa.extend(o.updates);
    }
}

/// Things to publish after the core lock is released.
#[derive(Default)]
struct Out {
    events: Vec<AppEvent>,
    qa: Vec<QaUpdate>,
}

struct Inner {
    deps: SessionDeps,
    bus: EventBus,
    core: Mutex<Core>,
    qa_subs: Mutex<Vec<Sender<QaUpdate>>>,
    cancel: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    demo_done: AtomicBool,
}

pub struct Session {
    inner: Arc<Inner>,
}

struct SessionSink(Weak<Inner>);
impl SttSink for SessionSink {
    fn emit(&self, ev: SttEvent) {
        if let Some(i) = self.0.upgrade() {
            i.ingest(ev);
        }
    }
    fn error(&self, e: SttError) {
        if let Some(i) = self.0.upgrade() {
            tracing::warn!(?e, "stt error");
            i.core().stt_errors.push(e);
        }
    }
}

impl Session {
    pub fn new(deps: SessionDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                deps,
                bus: EventBus::default(),
                core: Mutex::new(Core::new()),
                qa_subs: Mutex::new(vec![]),
                cancel: Arc::new(AtomicBool::new(false)),
                paused: Arc::new(AtomicBool::new(false)),
                demo_done: AtomicBool::new(false),
            }),
        }
    }

    /// Bus events (state, stt segments, llm stream, levels).
    pub fn subscribe(&self) -> Receiver<AppEvent> {
        self.inner.bus.subscribe()
    }
    pub fn add_sink(&self, sink: Arc<dyn EventSink>) {
        self.inner.bus.add_sink(sink)
    }
    /// «Мой ответ» updates (text so far / closed). See CONTRACT-CHANGES.md.
    pub fn subscribe_qa(&self) -> Receiver<QaUpdate> {
        let (tx, rx) = channel();
        self.inner.qa_subs.lock().unwrap().push(tx);
        rx
    }

    pub fn state(&self) -> SessionState {
        self.inner.core().state.clone()
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let c = self.inner.core();
        SessionSnapshot {
            state: c.state.clone(),
            clock: c.clock,
            transcript: c.transcript.lines(),
            queue: QueueState::default(),
        }
    }

    /// Feeds an STT event as if an engine emitted it (tests, dev tools).
    #[doc(hidden)]
    pub fn inject_stt(&self, ev: SttEvent) {
        self.inner.ingest(ev);
    }

    pub fn qa_records(&self) -> Vec<QaRecord> {
        self.inner.core().qa.records().to_vec()
    }
    pub fn my_answer(&self, qa: &QaId) -> Option<String> {
        self.inner.core().qa.my_answer(qa).map(str::to_owned)
    }
    pub fn last_call_meta(&self) -> Option<CallMeta> {
        self.inner.core().last_meta.clone()
    }
    pub fn stt_errors(&self) -> Vec<SttError> {
        self.inner.core().stt_errors.clone()
    }

    /// Blocks until the demo script has played to the end (or was cancelled).
    pub fn wait_demo(&self) {
        let h = self.inner.core().demo_thread.take();
        if let Some(h) = h {
            let _ = h.join();
        }
    }
    pub fn demo_finished(&self) -> bool {
        self.inner.demo_done.load(Ordering::SeqCst)
    }

    pub fn start(&self, o: StartOpts) -> Result<CallId> {
        let inner = &self.inner;
        let settings = inner.deps.settings.get();
        let t0_mono = now_mono_ns();
        let t0_wall_ms = wall_ms();
        let clock = SessionClock { t0_mono, t0_wall_ms };
        let mut out = Out::default();
        {
            let mut c = inner.core();
            if c.state.phase != SessionPhase::Idle {
                return Err(AppError::new(ErrorCode::SessionBusy));
            }
            *c = Core {
                last_meta: None,
                last_materialize: t0_mono,
                range_from: t0_mono,
                clock: Some(clock),
                state: c.state.clone(),
                ..Core::new()
            };
            c.state.demo = o.demo;
            c.state.call = None;
            c.set_phase(SessionPhase::Starting, &mut out);
        }
        inner.cancel.store(false, Ordering::SeqCst);
        inner.paused.store(false, Ordering::SeqCst);
        inner.demo_done.store(false, Ordering::SeqCst);
        inner.flush(out);

        let call_id = match inner.setup(&o, &settings) {
            Ok(id) => id,
            Err(e) => {
                let mut out = Out::default();
                inner.teardown();
                let rec = inner.core().recorder.take();
                if let Some(r) = rec {
                    let _ = r.finish();
                }
                inner.core().set_phase(SessionPhase::Idle, &mut out);
                inner.flush(out);
                return Err(e);
            }
        };
        let mut out = Out::default();
        {
            let mut c = inner.core();
            c.state.call = Some(call_id);
            c.set_phase(SessionPhase::Recording, &mut out);
        }
        inner.flush(out);
        if o.demo {
            let me = inner.clone();
            let h = std::thread::Builder::new()
                .name("lowk-demo".into())
                .spawn(move || me.run_demo(t0_mono, t0_wall_ms))
                .expect("spawn demo thread");
            inner.core().demo_thread = Some(h);
        }
        Ok(call_id)
    }

    pub fn pause(&self) -> Result<()> {
        self.inner.pause_resume(true)
    }
    pub fn resume(&self) -> Result<()> {
        self.inner.pause_resume(false)
    }

    pub fn stop(&self) -> Result<()> {
        let inner = &self.inner;
        let mut out = Out::default();
        {
            let mut c = inner.core();
            match c.state.phase {
                SessionPhase::Idle => return Err(AppError::new(ErrorCode::SessionIdle)),
                SessionPhase::Starting | SessionPhase::Stopping => {
                    return Err(AppError::new(ErrorCode::SessionBusy))
                }
                _ => c.set_phase(SessionPhase::Stopping, &mut out),
            }
        }
        inner.flush(out);

        // Stop sources first (engines flush their open utterance -> finals).
        inner.cancel.store(true, Ordering::SeqCst);
        inner.teardown();
        let (demo, threads) = {
            let mut c = inner.core();
            (c.demo_thread.take(), std::mem::take(&mut c.threads))
        };
        for h in demo.into_iter().chain(threads) {
            let _ = h.join();
        }

        let mut out = Out::default();
        let recorder = {
            let mut c = inner.core();
            let o = c.qa.close(crate::qa::CloseReason::SessionEnd);
            c.absorb_qa(o, &mut out);
            c.recorder.take()
        };
        let res = match recorder {
            Some(r) => r.finish(),
            None => Ok(None),
        };
        {
            let mut c = inner.core();
            if let Ok(m) = &res {
                c.last_meta = m.clone();
            }
            c.state.call = None;
            c.state.demo = false;
            c.set_phase(SessionPhase::Idle, &mut out);
        }
        inner.flush(out);
        res.map(|_| ())
    }

    /// Ask the AI. Returns the new Q&A id at once; the answer streams as
    /// `AppEvent::Llm` events from a worker thread.
    pub fn ask(&self, req: AskRequest) -> Result<QaId> {
        let inner = self.inner.clone();
        let settings = inner.deps.settings.get();
        let template = settings
            .templates
            .iter()
            .find(|t| t.id == req.template_id)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("template"))?;
        let qa_id = Ulid::new();
        let req_id = Ulid::new();
        let (delta, prev_cursor, question, my_answer) = {
            let mut c = inner.core();
            match c.state.phase {
                SessionPhase::Recording | SessionPhase::Paused => {}
                SessionPhase::Idle => return Err(AppError::new(ErrorCode::SessionIdle)),
                _ => return Err(AppError::new(ErrorCode::SessionBusy)),
            }
            let delta = c.transcript.since(c.llm_cursor);
            let prev = c.llm_cursor;
            c.llm_cursor = c.transcript.next_seq();
            let question = req
                .typed_text
                .clone()
                .filter(|t| !t.trim().is_empty())
                .or_else(|| {
                    let pick = |sys_only: bool| {
                        delta
                            .iter()
                            .rev()
                            .find(|l| l.is_final && (!sys_only || l.stream == Stream::System))
                            .map(|l| l.text.clone())
                    };
                    pick(true).or_else(|| pick(false))
                });
            let my = if template.needs_my_answer {
                c.qa.latest_my_answer().map(str::to_owned)
            } else {
                None
            };
            (delta, prev, question.unwrap_or_default(), my)
        };
        let context = inner
            .deps
            .retriever
            .retrieve(&RetrievalQuery {
                text: question.clone(),
                scope: RetrievalScope {
                    folder: None,
                    call: None,
                    tags: vec![],
                    all_folders: false,
                },
                budget_tokens: 2000,
                exclude: vec![],
                cloud_provider: false,
            })
            .unwrap_or_default();
        inner.qa_begin(qa_id, req_id, &question);
        let hint_req = HintRequest {
            qa_id,
            template_id: template.id.clone(),
            question: question.clone(),
            typed_text: req.typed_text,
            delta,
            context,
            my_answer,
        };
        let me = inner.clone();
        let h = std::thread::Builder::new()
            .name("lowk-ask".into())
            .spawn(move || {
                let mut deltas = |t: &str| {
                    me.bus.emit(AppEvent::Llm(LlmEvent::Delta {
                        req_id,
                        text: t.to_owned(),
                    }));
                };
                match me.deps.hints.generate(&hint_req, &mut deltas) {
                    Ok(answer) => me.qa_finish(
                        qa_id,
                        req_id,
                        &template.id,
                        &me.deps.hints.model(),
                        answer,
                        now_mono_ns(),
                    ),
                    Err(error) => {
                        me.core().llm_cursor = prev_cursor;
                        me.core().pending_q.remove(&qa_id);
                        me.bus.emit(AppEvent::Llm(LlmEvent::Error { req_id, error }));
                    }
                }
            })
            .expect("spawn ask thread");
        inner.core().threads.push(h);
        Ok(qa_id)
    }
}

impl Inner {
    fn core(&self) -> MutexGuard<'_, Core> {
        self.core.lock().unwrap()
    }

    fn flush(&self, out: Out) {
        self.bus.emit_all(out.events);
        if !out.qa.is_empty() {
            let mut subs = self.qa_subs.lock().unwrap();
            for u in out.qa {
                subs.retain(|s| s.send(u.clone()).is_ok());
            }
        }
    }

    fn pause_resume(&self, pause: bool) -> Result<()> {
        let mut out = Out::default();
        {
            let mut c = self.core();
            let (from, to) = if pause {
                (SessionPhase::Recording, SessionPhase::Paused)
            } else {
                (SessionPhase::Paused, SessionPhase::Recording)
            };
            match c.state.phase {
                p if p == from => {}
                SessionPhase::Idle => return Err(AppError::new(ErrorCode::SessionIdle)),
                _ => return Err(AppError::new(ErrorCode::SessionBusy)),
            }
            self.paused.store(pause, Ordering::SeqCst);
            c.set_phase(to, &mut out);
        }
        self.flush(out);
        Ok(())
    }

    /// Opens the call, engines and capture (or just the call for demo).
    fn setup(self: &Arc<Self>, o: &StartOpts, settings: &Settings) -> Result<CallId> {
        let clock = self.core().clock.expect("clock set in start");
        let wall = clock.t0_wall_ms;
        let (me_label, sys_label) = match settings.ui.language {
            UiLanguage::En => ("Me", "System"),
            _ => ("Я", "Система"),
        };
        let system = o.system.clone().or_else(|| settings.audio.default_source.clone());
        let mic = o.mic.clone().or_else(|| settings.audio.mic_device.clone());
        let engine_id = if o.demo {
            "demo".to_string()
        } else if settings.stt.engine.is_empty() {
            self.deps
                .stt
                .available()
                .first()
                .map(|d| d.id.clone())
                .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("no stt engine"))?
        } else {
            settings.stt.engine.clone()
        };
        let new_call = NewCall {
            title: o.title.clone().unwrap_or_else(|| default_title(wall, o.demo)),
            folder: o.folder.clone().unwrap_or(FolderRef::Unsorted),
            started: rfc3339(wall),
            sources: CallSources {
                me: mic.clone(),
                system: system.as_ref().map(|t| match t {
                    Target::App(id) => id.clone(),
                    Target::WholeSystem => "system".into(),
                }),
            },
            stt_engine: Some(engine_id.clone()),
            languages: settings.stt.languages.clone(),
            speaker_me: me_label.into(),
            speaker_system: sys_label.into(),
        };

        if o.demo {
            if self.deps.record_demo {
                let (id, rec) = self.deps.store.begin_call(new_call)?;
                self.core().recorder = Some(rec);
                return Ok(id);
            }
            return Ok(Ulid::from_parts(wall as u64, 0));
        }

        let (id, rec) = self.deps.store.begin_call(new_call)?;
        self.core().recorder = Some(rec);

        let mut engines = HashMap::new();
        for stream in [Stream::Me, Stream::System] {
            let mut eng = self.deps.stt.create(&engine_id, stream)?;
            eng.start(
                &SttConfig {
                    stream,
                    langs: settings.stt.languages.clone(),
                    vocab_hints: settings.stt.vocab_hints.clone(),
                },
                Arc::new(SessionSink(Arc::downgrade(self))),
            )?;
            engines.insert(stream, Arc::new(Mutex::new(eng)));
        }
        let engines: Engines = Arc::new(engines);
        self.core().engines = Some(engines.clone());

        let paused = self.paused.clone();
        let frame_sink: FrameSink = Arc::new(move |f: Frame16k| {
            if paused.load(Ordering::Relaxed) {
                return;
            }
            if let Some(e) = engines.get(&f.stream) {
                if let Err(err) = e.lock().unwrap().push(&f) {
                    tracing::warn!(?err, "stt push failed");
                }
            }
        });
        let weak = Arc::downgrade(self);
        let ev_sink = Arc::new(move |e: AudioEvent| match e {
            AudioEvent::Level { me, sys } => {
                if let Some(i) = weak.upgrade() {
                    i.bus.emit(AppEvent::Level(AudioLevel { me, sys }));
                }
            }
            other => tracing::warn!(?other, "audio event"),
        });
        let handle = self.deps.audio.lock().unwrap().start(
            &CaptureConfig {
                system,
                mic,
                aec: settings.audio.aec,
            },
            frame_sink,
            ev_sink,
        )?;
        self.core().capture = Some(handle);
        Ok(id)
    }

    /// Stops capture, then flushes and stops the engines. The recorder stays
    /// in place so the final utterances they emit are still journaled.
    fn teardown(&self) {
        let (capture, engines) = {
            let mut c = self.core();
            (c.capture.take(), c.engines.take())
        };
        drop(capture);
        if let Some(engines) = engines {
            for e in engines.values() {
                let mut e = e.lock().unwrap();
                let _ = e.flush();
                let _ = e.stop();
            }
        }
    }

    /// An STT event from an engine (or the demo feed, rebased).
    fn ingest(&self, ev: SttEvent) {
        let mut out = Out::default();
        {
            let mut c = self.core();
            if c.state.phase == SessionPhase::Idle {
                return;
            }
            let Some(clock) = c.clock else { return };
            if c.transcript.apply(&ev, &clock) == Applied::Stale {
                return;
            }
            out.events.push(AppEvent::Stt(ev.clone()));
            let qa = c.qa.on_stt(&ev);
            c.absorb_qa(qa, &mut out);
            if ev.kind == SttKind::Final {
                c.push_entry(CallEntry::Final { event: ev.clone() });
                if ev.t_emit_ns >= c.last_materialize + MATERIALIZE_EVERY_NS {
                    c.last_materialize = ev.t_emit_ns;
                    if let Some(r) = c.recorder.as_mut() {
                        let _ = r.materialize();
                    }
                }
            }
        }
        self.flush(out);
    }

    fn qa_begin(&self, qa_id: QaId, req_id: ReqId, question: &str) {
        let mut out = Out::default();
        {
            let mut c = self.core();
            let o = c.qa.on_ask();
            c.absorb_qa(o, &mut out);
            c.pending_q.insert(qa_id, question.to_owned());
        }
        out.events.push(AppEvent::Llm(LlmEvent::Start { req_id, qa_id }));
        self.flush(out);
    }

    fn qa_finish(
        &self,
        qa_id: QaId,
        req_id: ReqId,
        template: &str,
        model: &str,
        answer: String,
        armed_at: MonoNs,
    ) {
        let mut out = Out::default();
        {
            let mut c = self.core();
            let question = c.pending_q.remove(&qa_id).unwrap_or_default();
            if let Some(clock) = c.clock {
                let range = format!(
                    "{}-{}",
                    clock.offset_label(c.range_from),
                    clock.offset_label(armed_at)
                );
                c.range_from = armed_at;
                let at_s = (clock.offset_ns(armed_at) / 1_000_000_000) as u32;
                c.push_entry(CallEntry::Qa {
                    id: qa_id,
                    at_s,
                    template: template.into(),
                    model: model.into(),
                    range,
                    question: question.clone(),
                    answer: answer.clone(),
                });
            }
            let o = c.qa.arm(qa_id, question, answer, armed_at);
            c.absorb_qa(o, &mut out);
        }
        out.events.push(AppEvent::Llm(LlmEvent::Done {
            req_id,
            truncated: false,
            usage: None,
        }));
        self.flush(out);
    }

    fn run_demo(self: Arc<Self>, t0: MonoNs, wall: i64) {
        let pacer = Pacer {
            cancel: self.cancel.clone(),
            paused: self.paused.clone(),
            pace: self.deps.demo_pace,
        };
        let ids = |qa: u32| {
            (
                Ulid::from_parts(wall as u64, u128::from(qa)),
                Ulid::from_parts(wall as u64, 1000 + u128::from(qa)),
            )
        };
        let finished = ReplaySource::demo().run(&pacer, &mut |item: &ReplayItem| match &item.kind {
            ReplayKind::Stt(ev) => self.ingest(rebase(ev, t0)),
            ReplayKind::HintStart { qa, question } => {
                let (q, r) = ids(*qa);
                self.qa_begin(q, r, question);
            }
            ReplayKind::HintDelta { qa, text } => {
                self.bus.emit(AppEvent::Llm(LlmEvent::Delta {
                    req_id: ids(*qa).1,
                    text: text.clone(),
                }));
            }
            ReplayKind::HintDone { qa, answer } => {
                let (q, r) = ids(*qa);
                self.qa_finish(q, r, "brief", "demo", answer.clone(), t0 + item.at_ms * NS_PER_MS);
            }
        });
        if finished {
            self.demo_done.store(true, Ordering::SeqCst);
        }
    }
}

fn wall_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn rfc3339(wall_ms: i64) -> String {
    use ::time::format_description::well_known::Rfc3339;
    ::time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(wall_ms) * 1_000_000)
        .ok()
        .and_then(|t| t.format(&Rfc3339).ok())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".into())
}

fn default_title(wall_ms: i64, demo: bool) -> String {
    let d = rfc3339(wall_ms);
    let d = d.get(..16).unwrap_or(&d).replace('T', " ");
    if demo {
        format!("Demo {d}")
    } else {
        format!("Call {d}")
    }
}
