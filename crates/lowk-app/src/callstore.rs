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
}

impl CallRecorder for Recorder {
    fn push(&mut self, e: CallEntry) -> Result<()> {
        self.w.push(e)
    }
    fn materialize(&mut self) -> Result<()> {
        self.w.materialize()
    }
    fn finish(self: Box<Self>) -> Result<Option<CallMeta>> {
        let id = self.w.id();
        let meta = self.w.finish()?;
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
            }),
        ))
    }
}
