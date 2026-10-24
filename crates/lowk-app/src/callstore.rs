//! `CallStore` / `CallRecorder` over `lowk-store` (the session never sees the vault).
use lowk_session::{CallRecorder, CallStore};
use lowk_store::{CallWriter, Vault};
use lowk_types::*;
use parking_lot::RwLock;
use std::sync::Arc;

pub struct VaultCallStore {
    pub vault: Arc<RwLock<Option<Vault>>>,
    pub emitter: Arc<crate::emitter::UiEmitter>,
}

struct Recorder {
    w: CallWriter,
    emitter: Arc<crate::emitter::UiEmitter>,
    vault: Vault,
    /// `audio.keep_audio`: the call's frames go to `<vault>/.lowk/audio/<id>/`.
    keeper: Option<Arc<lowk_store::audio::AudioKeeper>>,
}

impl CallRecorder for Recorder {
    fn push(&mut self, e: CallEntry) -> Result<()> {
        self.w.push(e)
    }
    fn materialize(&mut self) -> Result<()> {
        self.w.materialize()
    }
    fn keep_audio(&mut self, clock: SessionClock) -> Option<lowk_session::AudioTap> {
        match self.vault.audio_keeper(&self.w.id(), clock.t0_mono) {
            Ok(k) => {
                let k = Arc::new(k);
                self.keeper = Some(k.clone());
                Some(Arc::new(move |f: &Frame16k| k.write(f)))
            }
            Err(e) => {
                tracing::warn!(?e, "cannot keep the call audio");
                None
            }
        }
    }
    fn finish(self: Box<Self>) -> Result<Option<CallMeta>> {
        let id = self.w.id();
        let kept = self.keeper.as_ref().is_some_and(|k| k.finish());
        let mut meta = self.w.finish()?;
        if kept {
            // the frontmatter records that audio exists (`audio: wav`); the files are the source of truth
            match self.vault.update_call(&id, |d| d.fm.audio = "wav".into()) {
                Ok(m) => meta = m,
                Err(e) => tracing::warn!(?e, "cannot mark the call as having audio"),
            }
        }
        use lowk_types::EventSink;
        self.emitter.emit(AppEvent::Store(StoreChange {
            kind: StoreChangeKind::Updated,
            id,
            is_folder: false,
        }));
        Ok(Some(meta))
    }
}

impl CallStore for VaultCallStore {
    fn begin_call(&self, new: NewCall) -> Result<(CallId, Box<dyn CallRecorder>)> {
        let vault = self
            .vault
            .read()
            .clone()
            .ok_or_else(|| AppError::new(ErrorCode::VaultNotOpen))?;
        let w = vault.begin_call(new)?;
        let id = w.id();
        use lowk_types::EventSink;
        self.emitter.emit(AppEvent::Store(StoreChange {
            kind: StoreChangeKind::Created,
            id,
            is_folder: false,
        }));
        Ok((
            id,
            Box::new(Recorder {
                w,
                emitter: self.emitter.clone(),
                vault,
                keeper: None,
            }),
        ))
    }
}
