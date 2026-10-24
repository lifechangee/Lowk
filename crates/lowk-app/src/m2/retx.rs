//! «Перераспознать запись»: the offline re-transcription of a call's saved audio.
//! Glue over `lowk_stt::retx` (the pass) and `lowk_store` (audio files, transcript
//! revisions). One pass at a time, in a background thread; it waits while a recording runs, can be
//! cancelled, and writes a NEW transcript revision (the live one is kept on disk, restorable).
//!
//! Local engine: Whisper large-v3-turbo in the sidecar worker (`correct` mode, lowest priority).
//! Cloud engine: Deepgram's pre-recorded API, only when `stt.retranscribe_cloud` is on and the call's
//! folder allows the cloud; the key comes from the existing store (`lowk_stt::cloud::resolve_key`).
use super::{folder_allows_cloud, Services};
use lowk_store::audio::{StreamAudio, WavReader};
use lowk_store::retx::{RetxChunk, RetxResult};
use lowk_stt::cloud::{resolve_key, Vendor};
use lowk_stt::retx::{self, DeepgramBatch, Hooks, Outcome, PcmSource, Transcriber, WhisperBatch};
use lowk_stt::{local, ModelStore};
use lowk_types::*;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Builds the engine of a pass. The app uses [`SystemRetxEngines`]; tests inject fakes.
pub trait RetxEngines: Send + Sync {
    fn make(
        &self,
        backend: RetxBackend,
        langs: &[String],
        vocab: &[String],
    ) -> Result<Box<dyn Transcriber + Send>>;
}

pub struct SystemRetxEngines;

impl RetxEngines for SystemRetxEngines {
    fn make(
        &self,
        backend: RetxBackend,
        langs: &[String],
        vocab: &[String],
    ) -> Result<Box<dyn Transcriber + Send>> {
        match backend {
            RetxBackend::Cloud => {
                let key = resolve_key(Vendor::Deepgram)
                    .ok()
                    .filter(|k| !k.trim().is_empty())
                    .ok_or_else(|| AppError::new(ErrorCode::ApiKeyMissing).with_detail("deepgram"))?;
                Ok(Box::new(DeepgramBatch::new(key, langs)))
            }
            RetxBackend::Local => {
                let store =
                    ModelStore::default_location().ok_or_else(|| AppError::new(ErrorCode::EngineNotReady))?;
                let exe = local::find_worker()
                    .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("lowk-stt-worker"))?;
                if !local::whisper_installed(&store) {
                    return Err(AppError::new(ErrorCode::EngineNotReady)
                        .with_detail("whisper large-v3-turbo is not installed"));
                }
                let model = store.path(local::WHISPER_ID, local::WHISPER_FILE);
                let mut args = vec![
                    "correct".to_string(),
                    "--model".into(),
                    model.to_string_lossy().into_owned(),
                    "--threads".into(),
                    "4".into(),
                ];
                // the user's vocabulary biases Whisper towards names and terms
                let prompt: String = vocab.join(", ").chars().take(220).collect();
                if !prompt.trim().is_empty() {
                    args.push("--prompt".into());
                    args.push(prompt);
                }
                Ok(Box::new(WhisperBatch::new(lowk_stt::sidecar::SidecarSpec {
                    exe,
                    args,
                    nice: 19,
                })))
            }
        }
    }
}

struct Job {
    call: CallId,
    cancel: Arc<AtomicBool>,
    /// `(paused, done_s, total_s)`
    progress: Arc<Mutex<(bool, u32, u32)>>,
}

#[derive(Default)]
pub(crate) struct RetxState {
    job: Mutex<Option<Job>>,
    /// A call is being recorded: a running pass waits.
    recording: AtomicBool,
    engines: Mutex<Option<Arc<dyn RetxEngines>>>,
}

struct Pcm(WavReader);
impl PcmSource for Pcm {
    fn len_samples(&self) -> u64 {
        self.0.len_samples()
    }
    fn read(&mut self, start: u64, n: usize) -> Result<Vec<f32>> {
        Ok(self.0.read(start, n)?)
    }
}

impl Services {
    /// The session started / ended a recording: a running re-transcription pauses / resumes.
    pub fn retx_set_recording(&self, on: bool) {
        self.retx.recording.store(on, Ordering::SeqCst);
    }

    /// Replaces the engine factory (tests).
    pub fn retx_set_engines(&self, e: Arc<dyn RetxEngines>) {
        *self.retx.engines.lock() = Some(e);
    }

    fn retx_engines(&self) -> Arc<dyn RetxEngines> {
        self.retx
            .engines
            .lock()
            .clone()
            .unwrap_or_else(|| Arc::new(SystemRetxEngines))
    }

    /// What the call page needs: audio present, pass running / paused / progress, revisions.
    pub fn retx_status(&self, id: &CallId) -> Result<RetxStatus> {
        let doc = self.vault.read_call_doc(id)?;
        let mut st = RetxStatus {
            has_audio: !self.vault.call_audio(id).is_empty(),
            rev: lowk_store::retx::transcript_rev(&doc),
            revisions: self.vault.revisions(id),
            ..Default::default()
        };
        if let Some(j) = self.retx.job.lock().as_ref().filter(|j| j.call == *id) {
            let (paused, done, total) = *j.progress.lock();
            (st.running, st.paused, st.done_s, st.total_s) = (true, paused, done, total);
        }
        Ok(st)
    }

    /// Starts a pass over the call's saved audio. Errors that can be known up front (no audio, a
    /// local-only folder with the cloud engine, a missing model or key, a pass already running) are
    /// returned here; everything later arrives as `retx://event`.
    pub fn retx_start(self: &Arc<Self>, id: CallId, automatic: bool) -> Result<()> {
        let call = self.vault.read_call(&id)?;
        if call.meta.deleted {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_detail("call is in the trash"));
        }
        if call.meta.ended.is_none() {
            return Err(AppError::new(ErrorCode::SessionBusy).with_detail("call is still being recorded"));
        }
        let audio = self.vault.call_audio(&id);
        if audio.is_empty() {
            return Err(AppError::new(ErrorCode::NotFound).with_detail("no saved audio for this call"));
        }
        let s = self.settings();
        let backend = retx::pick_backend(
            s.stt.retranscribe_cloud,
            folder_allows_cloud(&self.vault, &call.meta.folder),
            automatic,
        )?;
        let langs = call.meta.languages.clone();
        let tr = self.retx_engines().make(backend, &langs, &s.stt.vocab_hints)?;
        let job = Job {
            call: id,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new((false, 0, 0))),
        };
        {
            let mut g = self.retx.job.lock();
            if g.is_some() {
                return Err(AppError::new(ErrorCode::SessionBusy)
                    .with_detail("a re-transcription is already running"));
            }
            *g = Some(Job {
                call: id,
                cancel: job.cancel.clone(),
                progress: job.progress.clone(),
            });
        }
        let svc = self.clone();
        let spawned = std::thread::Builder::new()
            .name("lowk-retx".into())
            .spawn(move || svc.retx_thread(job, audio, backend, tr, langs));
        if let Err(e) = spawned {
            *self.retx.job.lock() = None;
            return Err(AppError::new(ErrorCode::Unknown).with_detail(e.to_string()));
        }
        Ok(())
    }

    pub fn retx_cancel(&self, id: &CallId) {
        if let Some(j) = self.retx.job.lock().as_ref().filter(|j| j.call == *id) {
            j.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Sets an older transcript revision (the live one, typically) back as the current one.
    pub fn retx_restore(&self, id: &CallId, rev: u32) -> Result<()> {
        if self.retx.job.lock().as_ref().is_some_and(|j| j.call == *id) {
            return Err(AppError::new(ErrorCode::SessionBusy).with_detail("re-transcription is running"));
        }
        let started = self.vault.read_call_doc(id)?.fm.started;
        self.vault
            .restore_revision(id, rev, &lowk_store::now_in_offset_of(&started))?;
        self.env.emit(AppEvent::Store(StoreChange {
            kind: StoreChangeKind::Updated,
            id: *id,
            is_folder: false,
        }));
        Ok(())
    }

    /// A recording just ended: re-transcribe it when the setting (or its folder's preset) says so
    /// and the audio was kept. Never blocks; errors are logged (the manual action stays available).
    pub fn retx_after_recording(self: &Arc<Self>, id: CallId) {
        let svc = self.clone();
        let _ = std::thread::Builder::new()
            .name("lowk-retx-auto".into())
            .spawn(move || {
                // the writer finishes the file just after the session goes idle
                for _ in 0..50 {
                    match svc.vault.read_call(&id) {
                        Ok(c) if c.meta.ended.is_some() => break,
                        Ok(_) => std::thread::sleep(Duration::from_millis(300)),
                        Err(_) => return,
                    }
                }
                let Ok(call) = svc.vault.read_call(&id) else {
                    return;
                };
                let preset = match &call.meta.folder {
                    FolderRef::User(_) => svc
                        .vault
                        .folders()
                        .ok()
                        .and_then(|fs| fs.into_iter().find(|f| f.id == call.meta.folder))
                        .map(|f| f.retx)
                        .unwrap_or_default(),
                    _ => RetxPreset::Inherit,
                };
                if !preset.resolve(svc.settings().stt.retranscribe_after)
                    || svc.vault.call_audio(&id).is_empty()
                {
                    return;
                }
                if let Err(e) = svc.retx_start(id, true) {
                    tracing::warn!(code = ?e.code, "automatic re-transcription not started");
                    svc.env
                        .emit(AppEvent::Retx(RetxEvent::Error { call: id, error: e }));
                }
            });
    }

    fn retx_thread(
        self: Arc<Self>,
        job: Job,
        audio: Vec<StreamAudio>,
        backend: RetxBackend,
        mut tr: Box<dyn Transcriber + Send>,
        langs: Vec<String>,
    ) {
        let id = job.call;
        let total_s = audio.iter().map(|a| a.seconds().ceil() as u32).max().unwrap_or(0);
        self.env.emit(AppEvent::Retx(RetxEvent::Start {
            call: id,
            total_s,
            backend,
        }));
        let r = self.retx_run(&job, &audio, &mut *tr, &langs);
        tr.release();
        *self.retx.job.lock() = None;
        let ev = match r {
            Ok(Some((rev, chunks))) => {
                self.env.emit(AppEvent::Store(StoreChange {
                    kind: StoreChangeKind::Updated,
                    id,
                    is_folder: false,
                }));
                RetxEvent::Done {
                    call: id,
                    rev,
                    chunks,
                }
            }
            Ok(None) => RetxEvent::Cancelled { call: id },
            Err(error) => RetxEvent::Error { call: id, error },
        };
        self.env.emit(AppEvent::Retx(ev));
    }

    /// `Some((revision, re-transcribed chunks))`, or `None` when cancelled.
    fn retx_run(
        &self,
        job: &Job,
        audio: &[StreamAudio],
        tr: &mut dyn Transcriber,
        langs: &[String],
    ) -> Result<Option<(u32, u32)>> {
        let id = job.call;
        let mut readers: Vec<(Stream, Pcm)> = vec![];
        for a in audio {
            readers.push((a.stream, Pcm(WavReader::open(&a.path)?)));
        }
        let mut srcs: Vec<(Stream, &mut dyn PcmSource)> = readers
            .iter_mut()
            .map(|(s, p)| (*s, p as &mut dyn PcmSource))
            .collect();
        let (cancel, recording) = (job.cancel.clone(), &self.retx.recording);
        let cancelled = || cancel.load(Ordering::SeqCst);
        let paused = || recording.load(Ordering::SeqCst);
        let prog = job.progress.clone();
        let mut last_s = u32::MAX;
        let mut progress = |done: u64, total: u64| {
            let (d, t) = ((done / 1000) as u32, (total / 1000) as u32);
            prog.lock().1 = d;
            prog.lock().2 = t;
            if d != last_s {
                last_s = d;
                self.env.emit(AppEvent::Retx(RetxEvent::Progress {
                    call: id,
                    done_s: d,
                    total_s: t,
                }));
            }
        };
        let prog2 = job.progress.clone();
        let mut pause_changed = |on: bool| {
            prog2.lock().0 = on;
            self.env.emit(AppEvent::Retx(if on {
                RetxEvent::Paused { call: id }
            } else {
                RetxEvent::Resumed { call: id }
            }));
        };
        let mut hooks = Hooks {
            cancelled: &cancelled,
            paused: &paused,
            progress: &mut progress,
            pause_changed: &mut pause_changed,
            poll: Duration::from_millis(500),
        };
        let pieces = match retx::run(&mut srcs, tr, langs, &mut hooks)? {
            Outcome::Cancelled => return Ok(None),
            Outcome::Done(p) => p,
        };
        let changed = pieces.iter().filter(|p| p.text.is_some()).count() as u32;
        let res = RetxResult {
            engine: tr.name(),
            chunks: pieces
                .into_iter()
                .map(|p| RetxChunk {
                    stream: p.stream,
                    start_ms: p.start_ms,
                    end_ms: p.end_ms,
                    text: p.text,
                })
                .collect(),
        };
        if changed == 0 {
            // nothing usable came back: no new revision, the live transcript stays as it is
            let rev = lowk_store::retx::transcript_rev(&self.vault.read_call_doc(&id)?);
            return Ok(Some((rev, 0)));
        }
        let started = self.vault.read_call_doc(&id)?.fm.started;
        let rev = self
            .vault
            .apply_retranscript(&id, &res, &lowk_store::now_in_offset_of(&started))?;
        Ok(Some((rev, changed)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m2::{Env, StaticKey};
    use lowk_store::{FileSettingsStore, Vault};
    use lowk_types::fakes::CollectEventSink;
    use std::sync::atomic::AtomicUsize;

    struct FakeEngines {
        text: &'static str,
        made: AtomicUsize,
        delay: Duration,
    }
    struct FakeTr(&'static str, Duration);
    impl Transcriber for FakeTr {
        fn name(&self) -> String {
            "fake-whisper".into()
        }
        fn transcribe(&mut self, _pcm: &[f32], _lang: Option<&str>) -> Result<String> {
            std::thread::sleep(self.1);
            Ok(self.0.into())
        }
    }
    impl RetxEngines for FakeEngines {
        fn make(
            &self,
            backend: RetxBackend,
            _l: &[String],
            _v: &[String],
        ) -> Result<Box<dyn Transcriber + Send>> {
            assert_eq!(backend, RetxBackend::Local, "no test here may reach the cloud");
            self.made.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeTr(self.text, self.delay)))
        }
    }

    struct Rig {
        svc: Arc<Services>,
        sink: Arc<CollectEventSink>,
        settings: Arc<FileSettingsStore>,
        dir: std::path::PathBuf,
    }
    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn rig(name: &str) -> Rig {
        let dir = std::env::temp_dir().join(format!("lowk-retx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let settings = Arc::new(FileSettingsStore::open(&dir.join("data")).unwrap());
        let sink = Arc::new(CollectEventSink::default());
        let env = Env {
            settings: settings.clone() as Arc<dyn SettingsStore>,
            sink: sink.clone(),
            keys: Arc::new(StaticKey(None)),
            models: lowk_index::embedder::models::ModelStore::new(dir.join("models")),
            embed_worker: None,
        };
        let vault = Vault::open(&dir.join("vault")).unwrap();
        Rig {
            svc: Services::start(env, vault).unwrap(),
            sink,
            settings,
            dir,
        }
    }

    /// A finished call with 12 s of "speech" saved on the System stream.
    fn call_with_audio(r: &Rig, folder: FolderRef, with_audio: bool) -> CallId {
        let clock = SessionClock {
            t0_mono: 0,
            t0_wall_ms: 0,
        };
        let mut w = r
            .svc
            .vault
            .begin_call_with_clock(
                NewCall {
                    title: "Lecture".into(),
                    folder,
                    started: "2026-10-24T10:00:00+03:00".into(),
                    sources: CallSources {
                        me: None,
                        system: None,
                    },
                    stt_engine: Some("nemotron-local".into()),
                    languages: vec!["ru".into()],
                    speaker_me: "Я".into(),
                    speaker_system: "Система".into(),
                    clock: Some(clock),
                },
                clock,
            )
            .unwrap();
        let id = w.id();
        w.push(CallEntry::Final {
            event: SttEvent {
                stream: Stream::System,
                utt: 1,
                rev: 1,
                kind: SttKind::Final,
                text: "живой текст".into(),
                lang: None,
                t_start_ns: 2_000_000_000,
                t_audio_end_ns: 4_000_000_000,
                t_emit_ns: 0,
            },
        })
        .unwrap();
        w.finish_at("2026-10-24T10:01:00+03:00".into()).unwrap();
        if with_audio {
            let k = r.svc.vault.audio_keeper(&id, 0).unwrap();
            for i in 0..600u64 {
                // 12 s of speech-level signal from t = 1 s .. 13 s
                k.write(&Frame16k {
                    stream: Stream::System,
                    t_mono_ns: 1_000_000_000 + i * 20_000_000,
                    pcm: vec![0.2; 320],
                });
            }
            assert!(k.finish());
        }
        id
    }

    fn wait_for(r: &Rig, f: impl Fn(&[AppEvent]) -> bool) -> Vec<AppEvent> {
        for _ in 0..400 {
            let evs = r.sink.0.lock().unwrap().clone();
            if f(&evs) {
                return evs;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("timed out: {:?}", r.sink.0.lock().unwrap());
    }

    fn retx_events(evs: &[AppEvent]) -> Vec<RetxEvent> {
        evs.iter()
            .filter_map(|e| match e {
                AppEvent::Retx(r) => Some(r.clone()),
                _ => None,
            })
            .collect()
    }

    fn engines(delay_ms: u64) -> Arc<FakeEngines> {
        Arc::new(FakeEngines {
            text: "Новый текст лекции.",
            made: AtomicUsize::new(0),
            delay: Duration::from_millis(delay_ms),
        })
    }

    #[test]
    fn a_pass_writes_a_new_revision_keeps_the_live_one_and_reindexes() {
        let r = rig("write");
        let id = call_with_audio(&r, FolderRef::Unsorted, true);
        r.svc.retx_set_engines(engines(0));
        let st = r.svc.retx_status(&id).unwrap();
        assert!(st.has_audio && !st.running && st.rev == 1 && st.revisions.is_empty());
        r.svc.retx_start(id, false).unwrap();
        let evs = wait_for(&r, |e| {
            retx_events(e).iter().any(|x| matches!(x, RetxEvent::Done { .. }))
        });
        let re = retx_events(&evs);
        assert!(
            matches!(
                re[0],
                RetxEvent::Start {
                    backend: RetxBackend::Local,
                    total_s: 13,
                    ..
                }
            ),
            "{re:?}"
        );
        assert!(re.iter().any(|e| matches!(e, RetxEvent::Progress { .. })));
        assert!(
            matches!(
                re.last().unwrap(),
                RetxEvent::Done {
                    rev: 2,
                    chunks: 1,
                    ..
                }
            ),
            "{re:?}"
        );
        assert!(
            evs.iter()
                .any(|e| matches!(e, AppEvent::Store(c) if c.id == id && c.kind == StoreChangeKind::Updated)),
            "the index is told to re-read the call"
        );
        let doc = r.svc.vault.read_call_doc(&id).unwrap();
        let texts: Vec<String> = doc
            .blocks()
            .iter()
            .filter_map(|b| match b {
                lowk_store::call_codec::Block::Turn { at_s, text, .. } => Some(format!("{at_s}:{text}")),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            ["0:Новый текст лекции."],
            "timestamp = start of the chunk (1 s minus context)"
        );
        let st = r.svc.retx_status(&id).unwrap();
        assert_eq!((st.rev, st.running, st.revisions.len()), (2, false, 1));
        assert_eq!(st.revisions[0].kind, "live");
        // the live text is restorable
        r.svc.retx_restore(&id, 1).unwrap();
        let live = r.svc.vault.read_call(&id).unwrap().md;
        assert!(live.contains("живой текст") && !live.contains("Новый текст"));
    }

    #[test]
    fn cancel_leaves_the_call_untouched() {
        let r = rig("cancel");
        let id = call_with_audio(&r, FolderRef::Unsorted, true);
        r.svc.retx_set_engines(engines(300));
        let before = r.svc.vault.read_call(&id).unwrap().md;
        r.svc.retx_start(id, false).unwrap();
        assert!(r.svc.retx_status(&id).unwrap().running);
        assert_eq!(
            r.svc.retx_start(id, false).unwrap_err().code,
            ErrorCode::SessionBusy,
            "one pass at a time"
        );
        r.svc.retx_cancel(&id);
        wait_for(&r, |e| {
            retx_events(e)
                .iter()
                .any(|x| matches!(x, RetxEvent::Cancelled { .. }))
        });
        assert_eq!(r.svc.vault.read_call(&id).unwrap().md, before);
        assert!(r.svc.retx_status(&id).unwrap().revisions.is_empty());
        assert!(!r.svc.retx_status(&id).unwrap().running);
    }

    #[test]
    fn a_recording_pauses_the_pass_and_the_end_of_it_resumes() {
        let r = rig("pause");
        let id = call_with_audio(&r, FolderRef::Unsorted, true);
        r.svc.retx_set_recording(true); // a recording is already running when the pass starts
        r.svc.retx_set_engines(engines(0));
        r.svc.retx_start(id, false).unwrap();
        wait_for(&r, |e| {
            retx_events(e)
                .iter()
                .any(|x| matches!(x, RetxEvent::Paused { .. }))
        });
        assert!(r.svc.retx_status(&id).unwrap().paused);
        std::thread::sleep(Duration::from_millis(700));
        assert!(
            !retx_events(&r.sink.0.lock().unwrap())
                .iter()
                .any(|x| matches!(x, RetxEvent::Done { .. })),
            "nothing is transcribed while recording"
        );
        r.svc.retx_set_recording(false);
        let evs = wait_for(&r, |e| {
            retx_events(e).iter().any(|x| matches!(x, RetxEvent::Done { .. }))
        });
        assert!(retx_events(&evs)
            .iter()
            .any(|x| matches!(x, RetxEvent::Resumed { .. })));
    }

    #[test]
    fn no_saved_audio_means_no_action() {
        let r = rig("noaudio");
        let id = call_with_audio(&r, FolderRef::Unsorted, false);
        r.svc.retx_set_engines(engines(0));
        assert!(!r.svc.retx_status(&id).unwrap().has_audio);
        assert_eq!(r.svc.retx_start(id, false).unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn cloud_option_never_reaches_a_local_only_folder() {
        let r = rig("localonly");
        let f = r
            .svc
            .vault
            .create_folder(NewFolder {
                title: "Private".into(),
                tags: vec![],
                allow_cloud: false,
                instructions: String::new(),
            })
            .unwrap();
        let id = call_with_audio(&r, f.id.clone(), true);
        r.settings
            .patch(SettingsPatch(
                serde_json::json!({ "stt": { "retranscribe_cloud": true } }),
            ))
            .unwrap();
        let fake = engines(0);
        r.svc.retx_set_engines(fake.clone());
        // manual run: refused before any engine is built, nothing leaves the machine
        let e = r.svc.retx_start(id, false).unwrap_err();
        assert_eq!(e.code, ErrorCode::LocalOnlyBlocked);
        assert_eq!(fake.made.load(Ordering::SeqCst), 0);
        // the automatic run stays local (FakeEngines asserts that no cloud engine is requested)
        r.svc.retx_start(id, true).unwrap();
        wait_for(&r, |e| {
            retx_events(e).iter().any(|x| matches!(x, RetxEvent::Done { .. }))
        });
        assert!(matches!(
            retx_events(&r.sink.0.lock().unwrap())[0],
            RetxEvent::Start {
                backend: RetxBackend::Local,
                ..
            }
        ));
        // a folder that allows the cloud, same setting: the cloud engine is requested
        let ok = call_with_audio(&r, FolderRef::Unsorted, true);
        struct WantsCloud(Arc<AtomicBool>);
        impl RetxEngines for WantsCloud {
            fn make(
                &self,
                b: RetxBackend,
                _: &[String],
                _: &[String],
            ) -> Result<Box<dyn Transcriber + Send>> {
                self.0.store(b == RetxBackend::Cloud, Ordering::SeqCst);
                Err(AppError::new(ErrorCode::ApiKeyMissing))
            }
        }
        let asked = Arc::new(AtomicBool::new(false));
        r.svc.retx_set_engines(Arc::new(WantsCloud(asked.clone())));
        assert_eq!(
            r.svc.retx_start(ok, false).unwrap_err().code,
            ErrorCode::ApiKeyMissing
        );
        assert!(asked.load(Ordering::SeqCst));
    }

    #[test]
    fn automatic_pass_follows_the_setting_and_the_folder_preset() {
        let r = rig("auto");
        let on = r
            .svc
            .vault
            .create_folder(NewFolder {
                title: "Lectures".into(),
                tags: vec![],
                allow_cloud: true,
                instructions: String::new(),
            })
            .unwrap();
        r.svc
            .vault
            .update_folder(
                &match on.id {
                    FolderRef::User(u) => u,
                    _ => unreachable!(),
                },
                lowk_store::FolderPatch {
                    retx: Some(RetxPreset::On),
                    ..Default::default()
                },
            )
            .unwrap();
        r.svc.retx_set_engines(engines(0));
        // global off + inherit: nothing starts
        let plain = call_with_audio(&r, FolderRef::Unsorted, true);
        r.svc.retx_after_recording(plain);
        std::thread::sleep(Duration::from_millis(500));
        assert!(retx_events(&r.sink.0.lock().unwrap()).is_empty());
        // the folder says on: starts although the global setting is off
        let lecture = call_with_audio(&r, on.id.clone(), true);
        r.svc.retx_after_recording(lecture);
        wait_for(&r, |e| {
            retx_events(e)
                .iter()
                .any(|x| matches!(x, RetxEvent::Done { call, .. } if *call == lecture))
        });
        // global on: an inheriting folder starts too
        r.settings
            .patch(SettingsPatch(
                serde_json::json!({ "stt": { "retranscribe_after": true } }),
            ))
            .unwrap();
        r.svc.retx_after_recording(plain);
        wait_for(&r, |e| {
            retx_events(e)
                .iter()
                .any(|x| matches!(x, RetxEvent::Done { call, .. } if *call == plain))
        });
    }
}
