//! Dependencies of the session, all behind traits so the whole call flow runs
//! without Tauri, audio hardware, a vault or a network.
use crate::replay::DemoPace;
use lowk_audio::{AudioBackend, NoopAudioBackend};
use lowk_stt::{Corrector, EngineRegistry, FakeEngineFactory};
use lowk_types::{
    CallEntry, CallId, CallMeta, ContextPack, FolderRef, Frame16k, LlmUsage, MainThread, MicPreset, NewCall,
    QaId, QueuedImage, Result, Retriever, SentImage, SessionClock, SettingsStore, SourceRef, TranscriptLine,
    Ulid,
};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Write side of one live call (implemented over `lowk_store::CallWriter`).
pub trait CallRecorder: Send {
    fn push(&mut self, e: CallEntry) -> Result<()>;
    /// Cheap, called every ~20 s of call time: flush the journal to `.md`.
    fn materialize(&mut self) -> Result<()>;
    fn finish(self: Box<Self>) -> Result<Option<CallMeta>>;
    /// Called once at call start when `audio.keep_audio` is on: where the call's 16 kHz frames should
    /// be saved. The tap is called from the capture thread for every frame that reaches the engines
    /// (so paused time and a muted microphone leave silence); it must be cheap. The recorder closes
    /// the files in `finish`. Default: audio is not kept.
    fn keep_audio(&mut self, _clock: SessionClock) -> Option<AudioTap> {
        None
    }
    /// Stores a screenshot file next to the call; returns the link used in the call (`![[<rel>]]`).
    /// Default: not stored.
    fn save_image(&mut self, _name: &str, _bytes: &[u8]) -> Result<String> {
        Err(lowk_types::AppError::new(lowk_types::ErrorCode::Unsupported))
    }
}

/// Receives the frames of a call that keeps its audio.
pub type AudioTap = Arc<dyn Fn(&Frame16k) + Send + Sync>;

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
    /// Frames handed to the recorder's audio tap (`keep_audio`), as `(stream, capture time)`.
    pub kept_frames: Arc<Mutex<Vec<(lowk_types::Stream, u64)>>>,
    /// Screenshots stored with the call: `(file name, size)`.
    pub images: Arc<Mutex<Vec<(String, usize)>>>,
}
struct MemRecorder {
    entries: Arc<Mutex<Vec<CallEntry>>>,
    mats: Arc<Mutex<u32>>,
    finished: Arc<Mutex<u32>>,
    kept: Arc<Mutex<Vec<(lowk_types::Stream, u64)>>>,
    images: Arc<Mutex<Vec<(String, usize)>>>,
}
impl CallRecorder for MemRecorder {
    fn save_image(&mut self, name: &str, bytes: &[u8]) -> Result<String> {
        self.images.lock().unwrap().push((name.to_string(), bytes.len()));
        Ok(format!("mem.assets/{name}"))
    }
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
    fn keep_audio(&mut self, _clock: SessionClock) -> Option<AudioTap> {
        let kept = self.kept.clone();
        Some(Arc::new(move |f: &Frame16k| {
            kept.lock().unwrap().push((f.stream, f.t_mono_ns))
        }))
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
                kept: self.kept_frames.clone(),
                images: self.images.clone(),
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
    /// Screenshots queued for this Ask (oldest first). They belong to the new user turn only.
    pub images: Vec<QueuedImage>,
    /// `Template.vision`: the template wants a vision model even when the queue is empty.
    pub vision_template: bool,
}

/// What a finished generation tells the session.
#[derive(Clone, Debug, Default)]
pub struct HintOutcome {
    pub answer: String,
    /// Chunk ids this turn carried (the session adds them to the call's sent set).
    pub sent_chunk_ids: Vec<String>,
    pub truncated: bool,
    pub usage: Option<LlmUsage>,
    /// The screenshots that went out with this answer (the session removes them from the queue and
    /// stores them with the call). Empty when the generator sent none.
    pub images: Vec<SentImage>,
}

/// The screenshot queue as the session sees it (implemented by the app over its in-memory queue).
pub trait ShotSource: Send + Sync {
    /// The queued screenshots, oldest first; the queue is left as it is.
    fn peek(&self) -> Vec<QueuedImage>;
    /// A vision template with an empty queue: grab the active display once (best effort, empty on any failure).
    fn capture_if_empty(&self) -> Vec<QueuedImage> {
        vec![]
    }
    /// Removes the screenshots with these ids (their Ask succeeded).
    fn consume(&self, ids: &[u64]);
    /// Empties the queue (the session stopped).
    fn clear(&self);
}

/// No screenshots at all.
#[derive(Default)]
pub struct NoShots;
impl ShotSource for NoShots {
    fn peek(&self) -> Vec<QueuedImage> {
        vec![]
    }
    fn consume(&self, _ids: &[u64]) {}
    fn clear(&self) {}
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
    /// Screenshots waiting for the next Ask.
    pub shots: Arc<dyn ShotSource>,
    pub demo_pace: DemoPace,
    /// Write demo calls to the store too (off by default; tests turn it on).
    pub record_demo: bool,
    /// The «record microphone» preset of a folder (the app reads it from `_folder.md`); inherit by default.
    pub folder_mic: Arc<dyn Fn(&FolderRef) -> MicPreset + Send + Sync>,
    /// «Microphone: auto» detection (does the call app hold an input stream?); unsupported by default.
    pub mic_probe: Arc<dyn lowk_audio::mic_use::MicUseProbe>,
    pub mic_auto: MicAutoCfg,
}

/// Timing of the auto microphone: poll period and hysteresis (on after the app held the microphone for
/// `on_after_ms`, off after it released it for `off_after_ms`). Tests shrink it.
#[derive(Clone, Copy, Debug)]
pub struct MicAutoCfg {
    pub poll_ms: u64,
    pub on_after_ms: u64,
    pub off_after_ms: u64,
}

impl Default for MicAutoCfg {
    fn default() -> Self {
        Self {
            poll_ms: 1000,
            on_after_ms: 1000,
            off_after_ms: 3000,
        }
    }
}

impl SessionDeps {
    /// The three required dependencies plus inert defaults for the rest (null store,
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
            shots: Arc::new(NoShots),
            demo_pace: DemoPace::Realtime(1.0),
            record_demo: false,
            folder_mic: Arc::new(|_| MicPreset::Inherit),
            mic_probe: Arc::new(lowk_audio::mic_use::NoMicUseProbe),
            mic_auto: MicAutoCfg::default(),
        }
    }
}
