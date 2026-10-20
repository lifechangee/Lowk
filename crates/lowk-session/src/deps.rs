//! Dependencies of the session, all behind traits so the whole call flow runs
//! without Tauri, audio hardware, a vault or a network.
use crate::replay::DemoPace;
use lowk_audio::{AudioBackend, NoopAudioBackend};
use lowk_stt::{Corrector, EngineRegistry, FakeEngineFactory};
use lowk_types::{
    CallEntry, CallId, CallMeta, ContextChunk, MainThread, NewCall, QaId, Result, Retriever, SettingsStore,
    TranscriptLine, Ulid,
};
use std::sync::{Arc, Mutex};

/// Write side of one live call (implemented over `lowk_store::CallWriter`).
pub trait CallRecorder: Send {
    fn push(&mut self, e: CallEntry) -> Result<()>;
    /// Cheap, called every ~20 s of call time: flush the journal to `.md`.
    fn materialize(&mut self) -> Result<()>;
    fn finish(self: Box<Self>) -> Result<Option<CallMeta>>;
}

/// Opens calls (implemented over `lowk_store::Vault::begin_call`).
pub trait CallStore: Send + Sync {
    fn begin_call(&self, new: NewCall) -> Result<(CallId, Box<dyn CallRecorder>)>;
}

/// Discards everything.
#[derive(Default)]
pub struct NullCallStore;
struct NullRecorder;
impl CallRecorder for NullRecorder {
    fn push(&mut self, _e: CallEntry) -> Result<()> {
        Ok(())
    }
    fn materialize(&mut self) -> Result<()> {
        Ok(())
    }
    fn finish(self: Box<Self>) -> Result<Option<CallMeta>> {
        Ok(None)
    }
}
impl CallStore for NullCallStore {
    fn begin_call(&self, _new: NewCall) -> Result<(CallId, Box<dyn CallRecorder>)> {
        Ok((Ulid::new(), Box::new(NullRecorder)))
    }
}

/// Keeps entries in memory (tests, dev tools).
#[derive(Default)]
pub struct MemCallStore {
    pub calls: Mutex<Vec<NewCall>>,
    pub entries: Arc<Mutex<Vec<CallEntry>>>,
    pub materializations: Arc<Mutex<u32>>,
    pub finished: Arc<Mutex<u32>>,
}
struct MemRecorder {
    entries: Arc<Mutex<Vec<CallEntry>>>,
    mats: Arc<Mutex<u32>>,
    finished: Arc<Mutex<u32>>,
}
impl CallRecorder for MemRecorder {
    fn push(&mut self, e: CallEntry) -> Result<()> {
        self.entries.lock().unwrap().push(e);
        Ok(())
    }
    fn materialize(&mut self) -> Result<()> {
        *self.mats.lock().unwrap() += 1;
        Ok(())
    }
    fn finish(self: Box<Self>) -> Result<Option<CallMeta>> {
        *self.finished.lock().unwrap() += 1;
        Ok(None)
    }
}
impl CallStore for MemCallStore {
    fn begin_call(&self, new: NewCall) -> Result<(CallId, Box<dyn CallRecorder>)> {
        self.calls.lock().unwrap().push(new);
        Ok((
            Ulid::new(),
            Box::new(MemRecorder {
                entries: self.entries.clone(),
                mats: self.materializations.clone(),
                finished: self.finished.clone(),
            }),
        ))
    }
}

/// Everything the LLM stub needs to answer one Ask.
#[derive(Clone, Debug)]
pub struct HintRequest {
    pub qa_id: QaId,
    pub template_id: String,
    pub question: String,
    pub typed_text: Option<String>,
    /// Transcript since the last successful Ask (incl. provisional lines).
    pub delta: Vec<TranscriptLine>,
    pub context: Vec<ContextChunk>,
    /// Set for templates with `needs_my_answer`: what the user said so far.
    pub my_answer: Option<String>,
}

/// Implemented over `lowk_llm::ChatClient` + `PromptBuilder` by the app.
pub trait HintGenerator: Send + Sync {
    /// Streams text deltas to `on_delta`, returns the full answer.
    fn generate(&self, req: &HintRequest, on_delta: &mut dyn FnMut(&str)) -> Result<String>;
    fn model(&self) -> String {
        "stub".into()
    }
}

/// Streams a fixed answer word by word.
pub struct StubHints(pub String);
impl Default for StubHints {
    fn default() -> Self {
        Self("Это подсказка-заглушка: подключите LLM-провайдера.".into())
    }
}
impl HintGenerator for StubHints {
    fn generate(&self, _req: &HintRequest, on_delta: &mut dyn FnMut(&str)) -> Result<String> {
        for w in self.0.split_inclusive(' ') {
            on_delta(w);
        }
        Ok(self.0.clone())
    }
}

pub struct SessionDeps {
    pub main_thread: Arc<dyn MainThread>,
    pub retriever: Arc<dyn Retriever>,
    pub settings: Arc<dyn SettingsStore>,
    pub store: Arc<dyn CallStore>,
    pub audio: Arc<Mutex<dyn AudioBackend>>,
    pub stt: Arc<EngineRegistry>,
    /// Background Whisper correction of finalized utterances (local engines); optional.
    pub corrector: Option<Arc<dyn Corrector>>,
    pub hints: Arc<dyn HintGenerator>,
    pub demo_pace: DemoPace,
    /// Write demo calls to the store too (off by default; tests turn it on).
    pub record_demo: bool,
}

impl SessionDeps {
    /// The three M0 dependencies plus inert defaults for the rest (null store,
    /// no-op audio, fake STT engine, stub hints, real-time demo).
    pub fn new(
        main_thread: Arc<dyn MainThread>,
        retriever: Arc<dyn Retriever>,
        settings: Arc<dyn SettingsStore>,
    ) -> Self {
        let mut reg = EngineRegistry::default();
        reg.register(Box::new(FakeEngineFactory));
        Self {
            main_thread,
            retriever,
            settings,
            store: Arc::new(NullCallStore),
            audio: Arc::new(Mutex::new(NoopAudioBackend)),
            stt: Arc::new(reg),
            corrector: None,
            hints: Arc::new(StubHints::default()),
            demo_pace: DemoPace::Realtime(1.0),
            record_demo: false,
        }
    }
}
