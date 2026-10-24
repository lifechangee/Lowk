//! Dependencies of the session, all behind traits so the whole call flow runs
//! without Tauri, audio hardware, a vault or a network.
use crate::replay::DemoPace;
use lowk_audio::{AudioBackend, NoopAudioBackend};
use lowk_stt::{Corrector, EngineRegistry, FakeEngineFactory};
use lowk_types::{
    CallEntry, CallId, CallMeta, ContextPack, FolderRef, LlmUsage, MainThread, MicPreset, NewCall, QaId,
    Result, Retriever, SettingsStore, SourceRef, TranscriptLine, Ulid,
};
use std::sync::atomic::AtomicBool;
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
    /// Retrieval result: folder instructions, pinned documents, relevant chunks of documents and
    /// previous calls of the folder (empty when nothing was found or the folder is Unsorted).
    pub pack: ContextPack,
    /// First Ask of this call: instructions and pinned documents belong into this turn; later
    /// turns carry only new chunks (the generator keeps the append-only conversation).
    pub first_turn: bool,
    /// Chunk ids already sent in this call (not resent, not listed as sources again).
    pub sent_chunk_ids: Vec<String>,
    pub folder: FolderRef,
    pub call: Option<CallId>,
    /// Set by `Session::cancel_ask`; generators stop streaming when it turns true.
    pub cancelled: Arc<AtomicBool>,
    /// Set for templates with `needs_my_answer`: what the user said so far.
    pub my_answer: Option<String>,
}

/// What a finished generation tells the session.
#[derive(Clone, Debug, Default)]
pub struct HintOutcome {
    pub answer: String,
    /// Chunk ids this turn carried (the session adds them to the call's sent set).
    pub sent_chunk_ids: Vec<String>,
    pub truncated: bool,
    pub usage: Option<LlmUsage>,
}

/// Implemented over `lowk_llm::ChatClient` + `PromptBuilder` by the app.
pub trait HintGenerator: Send + Sync {
    /// Streams text deltas to `on_delta`, returns the full answer.
    fn generate(&self, req: &HintRequest, on_delta: &mut dyn FnMut(&str)) -> Result<String>;
    /// Streaming variant used by the session: `on_sources` is called once, before the first delta.
    /// The default adapts `generate` (no sources).
    fn generate_ex(
        &self,
        req: &HintRequest,
        _on_sources: &mut dyn FnMut(Vec<SourceRef>, u32),
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<HintOutcome> {
        let answer = self.generate(req, on_delta)?;
        Ok(HintOutcome {
            answer,
            ..HintOutcome::default()
        })
    }
    /// Called before retrieval. `Ok(cloud)`: whether a request for `folder` leaves the machine
    /// (retrieval then withholds local-only folders). `Err`: the Ask is refused at once
    /// (`ApiKeyMissing`, `LocalOnlyBlocked`) and nothing is sent anywhere.
    fn preflight(&self, _folder: &FolderRef) -> Result<bool> {
        Ok(false)
    }
    fn model(&self) -> String {
        "stub".into()
    }
    /// Opens the provider connection ahead of the first Ask. Must send no user data.
    fn warm(&self, _folder: &FolderRef) {}
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
    /// Engine used when `settings.stt.engine` is empty (the hardware check's pick).
    pub default_engine: Option<String>,
    pub hints: Arc<dyn HintGenerator>,
    pub demo_pace: DemoPace,
    /// Write demo calls to the store too (off by default; tests turn it on).
    pub record_demo: bool,
    /// The «record microphone» preset of a folder (the app reads it from `_folder.md`); inherit by default.
    pub folder_mic: Arc<dyn Fn(&FolderRef) -> MicPreset + Send + Sync>,
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
            default_engine: None,
            hints: Arc::new(StubHints::default()),
            demo_pace: DemoPace::Realtime(1.0),
            record_demo: false,
            folder_mic: Arc::new(|_| MicPreset::Inherit),
        }
    }
}
