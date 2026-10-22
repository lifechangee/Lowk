//! The `Session` orchestrator and its state machine
//! (Idle -> Starting -> Recording <-> Paused -> Stopping -> Idle).
use crate::bus::EventBus;
use crate::deps::{CallRecorder, HintRequest, SessionDeps};
use crate::echo::EchoGuard;
use crate::qa::{ClosedAnswer, QaOutput, QaTracker};
use crate::replay::{rebase, Pacer, ReplayItem, ReplayKind, ReplaySource};
use crate::transcript::{Applied, TranscriptBuf};
use lowk_audio::{AudioEvent, CaptureConfig, CaptureHandle, FrameSink};
use lowk_stt::ring::PcmRing;
use lowk_stt::{CorrectionJob, CorrectionSink, Corrector, SttConfig, SttEngine};
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
type Rings = Arc<HashMap<Stream, Mutex<PcmRing>>>;

/// Seconds of audio kept per stream for background correction.
const RING_SECONDS: u32 = 90;
/// How long `stop` waits for outstanding corrections before keeping the live text.
const CORRECTION_GRACE: std::time::Duration = std::time::Duration::from_secs(4);

struct Core {
    state: SessionState,
    clock: Option<SessionClock>,
    transcript: TranscriptBuf,
    qa: QaTracker,
    /// S4: drops Me speech that is only the speakers' echo of System speech.
    echo: EchoGuard,
    recorder: Option<Box<dyn CallRecorder>>,
    capture: Option<Box<dyn CaptureHandle>>,
    engines: Option<Engines>,
    /// Background correction (local engines): audio rings, the worker, and finals whose
    /// journal write waits for the corrected text.
    rings: Option<Rings>,
    corrector: Option<Arc<dyn Corrector>>,
    corr_lang: Option<String>,
    allowed_langs: Vec<String>,
    pending_journal: HashMap<(Stream, UttId), SttEvent>,
    last_diag: Option<SttDiagnostics>,
    threads: Vec<JoinHandle<()>>,
    demo_thread: Option<JoinHandle<()>>,
    /// First transcript seq not yet sent to the LLM.
    llm_cursor: u64,
    /// Start of the transcript range the next Q&A covers (call-relative ns base).
    range_from: MonoNs,
    last_materialize: MonoNs,
    pending_q: HashMap<QaId, String>,
    /// Folder of the call being recorded (retrieval scope, local-only policy).
    folder: FolderRef,
    /// Chunk ids already sent to the model in this call, and whether a first Ask succeeded.
    sent_chunks: Vec<String>,
    asked: bool,
    last_meta: Option<CallMeta>,
    stt_errors: Vec<SttError>,
    /// A local engine is still loading its model (mirrors `AppEvent::SttLoading`).
    stt_loading: bool,
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
            echo: EchoGuard::default(),
            recorder: None,
            capture: None,
            engines: None,
            rings: None,
            corrector: None,
            corr_lang: None,
            allowed_langs: vec![],
            pending_journal: HashMap::new(),
            last_diag: None,
            threads: vec![],
            demo_thread: None,
            llm_cursor: 0,
            range_from: 0,
            last_materialize: 0,
            pending_q: HashMap::new(),
            folder: FolderRef::Unsorted,
            sent_chunks: vec![],
            asked: false,
            last_meta: None,
            stt_errors: vec![],
            stt_loading: false,
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
    weak: Weak<Inner>,
    deps: SessionDeps,
    bus: EventBus,
    core: Mutex<Core>,
    qa_subs: Mutex<Vec<Sender<QaUpdate>>>,
    cancel: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    demo_done: AtomicBool,
    /// Cancel flag of the Ask being generated.
    ask_cancel: Mutex<Option<Arc<AtomicBool>>>,
}

pub struct Session {
    inner: Arc<Inner>,
}

struct SessionSink(Weak<Inner>);
impl SttSink for SessionSink {
    fn emit(&self, ev: SttEvent) {
        if let Some(i) = self.0.upgrade() {
            i.ingest_inner(ev, true);
        }
    }
    fn error(&self, e: SttError) {
        if let Some(i) = self.0.upgrade() {
            tracing::warn!(?e, "stt error");
            i.core().stt_errors.push(e.clone());
            i.bus.emit(AppEvent::SttError(e));
        }
    }
}

impl CorrectionSink for SessionSink {
    fn done(&self, stream: Stream, utt: UttId, result: Option<SttEvent>) {
        if let Some(i) = self.0.upgrade() {
            i.apply_correction(stream, utt, result);
        }
    }
}

impl Session {
    pub fn new(deps: SessionDeps) -> Self {
        Self {
            inner: Arc::new_cyclic(|weak| Inner {
                weak: weak.clone(),
                deps,
                bus: EventBus::default(),
                core: Mutex::new(Core::new()),
                qa_subs: Mutex::new(vec![]),
                cancel: Arc::new(AtomicBool::new(false)),
                paused: Arc::new(AtomicBool::new(false)),
                demo_done: AtomicBool::new(false),
                ask_cancel: Mutex::new(None),
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
    /// «Мой ответ» updates (text so far / closed); also published as `AppEvent::MyAnswer`.
    pub fn subscribe_qa(&self) -> Receiver<QaUpdate> {
        let (tx, rx) = channel();
        self.inner.qa_subs.lock().unwrap().push(tx);
        rx
    }

    /// Pre-connects the LLM provider (TLS) for Asks in `folder`. Blocking; call from a thread.
    pub fn warm_hints(&self, folder: &FolderRef) {
        self.inner.deps.hints.warm(folder);
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

    /// Like [`Session::inject_stt`] but through the live-engine path (echo guard, correction).
    #[doc(hidden)]
    pub fn inject_engine_stt(&self, ev: SttEvent) {
        self.inner.ingest_inner(ev, true);
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
    /// Latency percentiles per stream (live while recording, last values afterwards).
    pub fn stt_diagnostics(&self) -> Option<SttDiagnostics> {
        let engines = self.inner.core().engines.clone();
        if let Some(e) = engines {
            self.inner.snapshot_diag(&e);
        }
        self.inner.core().last_diag.clone()
    }
    /// True while a local engine is still loading its model.
    pub fn stt_loading(&self) -> bool {
        self.inner.core().stt_loading
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
                folder: o.folder.clone().unwrap_or(FolderRef::Unsorted),
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
            let o = c.qa.close(CloseReason::SessionEnd);
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

    /// Stops the Ask being generated (the overlay shows no error for a cancel).
    pub fn cancel_ask(&self) {
        if let Some(f) = self.inner.ask_cancel.lock().unwrap().as_ref() {
            f.store(true, Ordering::SeqCst);
        }
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
        let (delta, prev_cursor, question, my_answer, folder, call, first_turn, sent) = {
            let mut c = inner.core();
            match c.state.phase {
                SessionPhase::Recording | SessionPhase::Paused => {}
                SessionPhase::Idle => return Err(AppError::new(ErrorCode::SessionIdle)),
                _ => return Err(AppError::new(ErrorCode::SessionBusy)),
            }
            let delta = c.transcript.snapshot_for_ask(c.llm_cursor);
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
                            // the line still being spoken counts: Ask never waits for a final
                            .find(|l| !sys_only || l.stream == Stream::System)
                            .map(|l| l.text.clone())
                    };
                    pick(true).or_else(|| pick(false))
                });
            let my = if template.needs_my_answer {
                c.qa.latest_my_answer().map(str::to_owned)
            } else {
                None
            };
            (
                delta,
                prev,
                question.unwrap_or_default(),
                my,
                c.folder.clone(),
                c.state.call,
                !c.asked,
                c.sent_chunks.clone(),
            )
        };
        inner.qa_begin(qa_id, req_id, &question, now_mono_ns());
        let cancelled = Arc::new(AtomicBool::new(false));
        *inner.ask_cancel.lock().unwrap() = Some(cancelled.clone());
        let typed_text = req.typed_text;
        let me = inner.clone();
        let h = std::thread::Builder::new()
            .name("lowk-ask".into())
            .spawn(move || {
                let fail = |error: AppError| {
                    let mut c = me.core();
                    c.llm_cursor = prev_cursor;
                    c.pending_q.remove(&qa_id);
                    c.qa.discard(&qa_id);
                    drop(c);
                    me.bus.emit(AppEvent::Llm(LlmEvent::Error { req_id, error }));
                };
                // Local-only policy and key checks come first: a refused Ask retrieves and sends nothing.
                let cloud = match me.deps.hints.preflight(&folder) {
                    Ok(c) => c,
                    Err(e) => return fail(e),
                };
                let pack = match &folder {
                    FolderRef::User(fid) => me
                        .deps
                        .retriever
                        .retrieve_pack(&RetrievalQuery {
                            text: question.clone(),
                            scope: RetrievalScope {
                                folder: Some(*fid),
                                call,
                                tags: vec![],
                                all_folders: false,
                            },
                            budget_tokens: settings.docs.retrieved_budget_tokens,
                            exclude: sent.clone(),
                            cloud_provider: cloud,
                            pinned_budget_tokens: settings.docs.pinned_budget_tokens,
                            include_folder_context: first_turn,
                            include_previous_calls: true,
                        })
                        .unwrap_or_else(|e| {
                            tracing::warn!(?e, "retrieval failed; asking without context");
                            ContextPack::default()
                        }),
                    _ => ContextPack::default(),
                };
                let hint_req = HintRequest {
                    qa_id,
                    template_id: template.id.clone(),
                    question: question.clone(),
                    typed_text,
                    delta,
                    pack,
                    first_turn,
                    sent_chunk_ids: sent,
                    folder: folder.clone(),
                    call,
                    cancelled,
                    my_answer,
                };
                let mut sources = |s: Vec<SourceRef>, withheld: u32| {
                    me.bus.emit(AppEvent::Llm(LlmEvent::Sources {
                        req_id,
                        sources: s,
                        withheld,
                    }));
                };
                let mut deltas = |t: &str| {
                    me.bus.emit(AppEvent::Llm(LlmEvent::Delta {
                        req_id,
                        text: t.to_owned(),
                    }));
                };
                match me.deps.hints.generate_ex(&hint_req, &mut sources, &mut deltas) {
                    Ok(out) => {
                        {
                            let mut c = me.core();
                            c.asked = true;
                            c.sent_chunks.extend(out.sent_chunk_ids.iter().cloned());
                        }
                        me.qa_finish(
                            qa_id,
                            req_id,
                            &template.id,
                            &me.deps.hints.model(),
                            out.answer,
                            out.truncated,
                            out.usage,
                            now_mono_ns(),
                        )
                    }
                    Err(error) => fail(error),
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
                self.bus.emit(AppEvent::MyAnswer(u));
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
            match &self.deps.default_engine {
                Some(id) => id.clone(),
                None => self
                    .deps
                    .stt
                    .available()
                    .first()
                    .map(|d| d.id.clone())
                    .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("no stt engine"))?,
            }
        } else {
            settings.stt.engine.clone()
        };
        // Languages of this call: the picked set (or the remembered / default one), limited to what the
        // engine decodes. Drives decoder hints, the foreign-letter guard, Whisper correction and the frontmatter.
        let engine_langs: Vec<String> = if o.demo {
            vec![]
        } else {
            self.deps
                .stt
                .factory(&engine_id)
                .map(|f| f.descriptor().languages)
                .unwrap_or_default()
        };
        let remembered = &settings.stt.last_languages;
        let requested: Option<&[String]> = o
            .languages
            .as_deref()
            .or((!remembered.is_empty()).then_some(remembered.as_slice()));
        let langs = lowk_types::langpolicy::resolve(requested, &settings.stt.languages, &engine_langs);
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
            languages: langs.clone(),
            speaker_me: me_label.into(),
            speaker_system: sys_label.into(),
            clock: Some(clock),
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
                    langs: langs.clone(),
                    vocab_hints: settings.stt.vocab_hints.clone(),
                },
                Arc::new(SessionSink(Arc::downgrade(self))),
            )?;
            engines.insert(stream, Arc::new(Mutex::new(eng)));
        }
        let engines: Engines = Arc::new(engines);
        self.core().engines = Some(engines.clone());
        self.watch_engine_loading(&engines);

        // Background correction (engine #2): local engines only, never required.
        let local = self
            .deps
            .stt
            .factory(&engine_id)
            .is_some_and(|f| f.descriptor().kind == EngineKind::Local);
        let mut rings: Option<Rings> = None;
        if local && settings.stt.background_correction {
            if let Some(corr) = self.deps.corrector.clone() {
                match corr.start() {
                    Ok(()) => {
                        rings = Some(Arc::new(
                            [Stream::Me, Stream::System]
                                .into_iter()
                                .map(|s| (s, Mutex::new(PcmRing::new(RING_SECONDS))))
                                .collect(),
                        ));
                        let mut c = self.core();
                        c.rings = rings.clone();
                        c.corrector = Some(corr);
                        c.allowed_langs = langs.clone();
                        c.corr_lang = match langs.as_slice() {
                            [one] => Some(one.clone()),
                            _ => None,
                        };
                    }
                    Err(e) => tracing::warn!(?e, "background correction unavailable"),
                }
            }
        }

        let paused = self.paused.clone();
        let frame_sink: FrameSink = Arc::new(move |f: Frame16k| {
            if paused.load(Ordering::Relaxed) {
                return;
            }
            if let Some(r) = rings.as_ref().and_then(|r| r.get(&f.stream)) {
                r.lock().unwrap().push(&f);
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
            AudioEvent::TargetLost => {
                tracing::warn!("audio target lost and not re-acquired within 3 s");
                if let Some(i) = weak.upgrade() {
                    i.bus.emit(AppEvent::AudioNotice(AudioNotice::TargetLost));
                }
            }
            AudioEvent::TargetRestored => {
                tracing::info!("audio target restored");
                if let Some(i) = weak.upgrade() {
                    i.bus.emit(AppEvent::AudioNotice(AudioNotice::TargetRestored));
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

    /// Publishes `AppEvent::SttLoading` while a local engine is still loading its model (a cold
    /// start takes seconds). Captured audio is queued by the engine meanwhile, so nothing is lost
    /// and the subtitles start the moment the model is up. Ends with the session.
    fn watch_engine_loading(self: &Arc<Self>, engines: &Engines) {
        if engines.values().all(|e| e.lock().unwrap().is_ready()) {
            return;
        }
        let me = Arc::downgrade(self);
        let engines = Arc::downgrade(engines);
        let spawned = std::thread::Builder::new()
            .name("lowk-stt-loading".into())
            .spawn(move || {
                let mut shown = false;
                loop {
                    let (Some(i), Some(es)) = (me.upgrade(), engines.upgrade()) else {
                        break;
                    };
                    let alive = i.core().engines.as_ref().is_some_and(|c| Arc::ptr_eq(c, &es));
                    let loading = alive && !es.values().all(|e| e.lock().unwrap().is_ready());
                    if loading != shown {
                        shown = loading;
                        i.core().stt_loading = loading;
                        i.bus.emit(AppEvent::SttLoading(loading));
                    }
                    if !alive {
                        break;
                    }
                    drop((i, es));
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(?e, "cannot watch engine loading");
        }
    }

    /// Stops capture, then flushes and stops the engines. The recorder stays
    /// in place so the final utterances they emit are still journaled.
    fn teardown(&self) {
        let (capture, engines) = {
            let mut c = self.core();
            (c.capture.take(), c.engines.take())
        };
        drop(capture);
        if let Some(engines) = &engines {
            self.snapshot_diag(engines);
            for e in engines.values() {
                let mut e = e.lock().unwrap();
                let _ = e.flush();
                let _ = e.stop();
            }
        }
        // Give outstanding corrections a moment, then keep whatever live text is left.
        let corr = self.core().corrector.take();
        if let Some(corr) = corr {
            corr.stop(CORRECTION_GRACE);
            let (corrected, skipped, lag) = corr.stats();
            if let Some(d) = self.core().last_diag.as_mut() {
                d.corrected = corrected;
                d.correction_skipped = skipped;
                d.correction_lag = lag;
            }
        }
        let mut c = self.core();
        c.rings = None;
        let left: Vec<SttEvent> = c.pending_journal.drain().map(|(_, v)| v).collect();
        for ev in left {
            c.push_entry(CallEntry::Final { event: ev });
        }
    }

    /// Latency counters of the engines, kept after the session ends.
    fn snapshot_diag(&self, engines: &Engines) {
        let mut streams = vec![];
        let (mut id, mut look) = (String::new(), 0);
        for (s, e) in engines.iter() {
            let e = e.lock().unwrap();
            id = e.descriptor().id.clone();
            look = look.max(e.lookahead_ms());
            if let Some(d) = e.stats() {
                streams.push(StreamDiag { stream: *s, ..d });
            }
        }
        streams.sort_by_key(|d| d.stream as u8);
        let (corrected, skipped, lag) = self
            .core()
            .corrector
            .as_ref()
            .map_or((0, 0, Percentiles::default()), |c| c.stats());
        self.core().last_diag = Some(SttDiagnostics {
            engine: id,
            lookahead_ms: look,
            streams,
            corrected,
            correction_skipped: skipped,
            correction_lag: lag,
        });
    }

    /// An STT event from the demo feed (rebased) or a test.
    fn ingest(&self, ev: SttEvent) {
        self.ingest_inner(ev, false);
    }

    /// An STT event; `from_engine` finals of local engines are queued for background
    /// correction (their journal write waits for the better text, but the UI and Ask
    /// see the live text at once).
    fn ingest_inner(&self, mut ev: SttEvent, from_engine: bool) {
        // S1: one choke point for every engine (and the demo feed): no «�», no control characters.
        ev.text = lowk_types::text::clean_speech(&ev.text);
        if ev.text.is_empty() {
            return;
        }
        let mut out = Out::default();
        let mut job = None;
        let corr;
        {
            let mut c = self.core();
            if c.state.phase == SessionPhase::Idle {
                return;
            }
            let Some(clock) = c.clock else { return };
            if from_engine && !echo_guard_off() {
                c.echo.note_system(&ev);
                match c.echo.filter_me(&ev) {
                    Some(kept) => {
                        if kept.text != ev.text {
                            tracing::info!(utt = ev.utt, kind = ?ev.kind, "echo guard trimmed Me speech");
                        }
                        ev = kept;
                    }
                    None => {
                        tracing::info!(utt = ev.utt, kind = ?ev.kind, chars = ev.text.chars().count(), "echo guard dropped Me speech");
                        return;
                    }
                }
            }
            if c.transcript.apply(&ev, &clock) == Applied::Stale {
                return;
            }
            out.events.push(AppEvent::Stt(ev.clone()));
            let qa = c.qa.on_stt(&ev);
            c.absorb_qa(qa, &mut out);
            corr = c.corrector.clone();
            if ev.kind == SttKind::Final {
                let cut = if from_engine && corr.is_some() {
                    c.rings
                        .as_ref()
                        .and_then(|r| r.get(&ev.stream))
                        .and_then(|r| r.lock().unwrap().cut(ev.t_start_ns, ev.t_audio_end_ns))
                } else {
                    None
                };
                if let Some(pcm) = cut {
                    c.pending_journal.insert((ev.stream, ev.utt), ev.clone());
                    job = Some(CorrectionJob {
                        stream: ev.stream,
                        utt: ev.utt,
                        rev: ev.rev,
                        t_start_ns: ev.t_start_ns,
                        t_audio_end_ns: ev.t_audio_end_ns,
                        pcm,
                        original: ev.text.clone(),
                        lang: c.corr_lang.clone(),
                        allowed: c.allowed_langs.clone(),
                    });
                } else {
                    c.push_entry(CallEntry::Final { event: ev.clone() });
                }
                if ev.t_emit_ns >= c.last_materialize + MATERIALIZE_EVERY_NS {
                    c.last_materialize = ev.t_emit_ns;
                    if let Some(r) = c.recorder.as_mut() {
                        let _ = r.materialize();
                    }
                }
            }
        }
        self.flush(out);
        if let (Some(job), Some(corr)) = (job, corr) {
            let backlog = self.backlog_ms(job.stream);
            let (stream, utt) = (job.stream, job.utt);
            let sink: Arc<dyn CorrectionSink> = Arc::new(SessionSink(self.weak.clone()));
            if !corr.submit(job, sink, backlog) {
                // skipped under load: the live text is final
                self.apply_correction(stream, utt, None);
            }
        }
    }

    fn backlog_ms(&self, stream: Stream) -> u32 {
        let engines = self.core().engines.clone();
        engines
            .and_then(|e| e.get(&stream).map(|e| e.lock().unwrap().backlog_ms()))
            .unwrap_or(0)
    }

    /// A corrected text for an already-final utterance (or `None`: keep the live text).
    fn apply_correction(&self, stream: Stream, utt: UttId, result: Option<SttEvent>) {
        let mut out = Out::default();
        {
            let mut c = self.core();
            let orig = c.pending_journal.remove(&(stream, utt));
            let clock = c.clock;
            let applied = match (&result, clock) {
                (Some(ev), Some(clock)) => c.transcript.apply(ev, &clock) != Applied::Stale,
                _ => false,
            };
            match (applied, result, orig) {
                (true, Some(ev), _) => {
                    out.events.push(AppEvent::Stt(ev.clone()));
                    c.push_entry(CallEntry::Final { event: ev });
                }
                (_, _, Some(o)) => c.push_entry(CallEntry::Final { event: o }),
                _ => {}
            }
        }
        self.flush(out);
    }

    fn qa_begin(&self, qa_id: QaId, req_id: ReqId, question: &str, armed_at: MonoNs) {
        let mut out = Out::default();
        {
            let mut c = self.core();
            // The link opens at Ask time, not when the hint finishes: the owner starts answering
            // while the hint streams, and that speech belongs to «Мой ответ».
            let o = c.qa.arm(qa_id, question.to_owned(), String::new(), armed_at);
            c.absorb_qa(o, &mut out);
            c.pending_q.insert(qa_id, question.to_owned());
        }
        out.events.push(AppEvent::Llm(LlmEvent::Start { req_id, qa_id }));
        self.flush(out);
    }

    #[allow(clippy::too_many_arguments)]
    fn qa_finish(
        &self,
        qa_id: QaId,
        req_id: ReqId,
        template: &str,
        model: &str,
        answer: String,
        truncated: bool,
        usage: Option<LlmUsage>,
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
            c.qa.set_hint(&qa_id, answer);
        }
        out.events.push(AppEvent::Llm(LlmEvent::Done {
            req_id,
            truncated,
            usage,
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
                self.qa_begin(q, r, question, t0 + item.at_ms * NS_PER_MS);
            }
            ReplayKind::HintDelta { qa, text } => {
                self.bus.emit(AppEvent::Llm(LlmEvent::Delta {
                    req_id: ids(*qa).1,
                    text: text.clone(),
                }));
            }
            ReplayKind::HintDone { qa, answer } => {
                let (q, r) = ids(*qa);
                self.qa_finish(
                    q,
                    r,
                    "brief",
                    "demo",
                    answer.clone(),
                    false,
                    None,
                    t0 + item.at_ms * NS_PER_MS,
                );
            }
        });
        if finished {
            self.demo_done.store(true, Ordering::SeqCst);
        }
    }
}

/// Diagnostics for measuring AEC alone: `LOWK_ECHO_GUARD=off` bypasses the transcript-level guard.
fn echo_guard_off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LOWK_ECHO_GUARD").is_ok_and(|v| v == "off"))
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
