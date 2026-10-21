//! Embedder: which one the index uses (`settings.embedder`), local model install with progress
//! (`engine://progress`, `engine == "embedder"`), warm-up of the sidecar, search commands.
use super::Services;
use lowk_index::embedder::{
    gemma_info, local, CloudEmbedder, LocalEmbedder, SharedEmbedder, GEMMA_ENGINE_ID,
};
use lowk_types::*;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct EmbedState {
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

impl Services {
    /// (Re)installs the embedder the settings ask for. Failures leave keyword search running and
    /// are shown through `SemanticState` (never a silent downgrade).
    pub fn apply_embedder(&self) {
        let s = self.settings();
        let (emb, state): (Option<SharedEmbedder>, SemanticState) = match s.embedder.mode {
            EmbedderMode::Off => (None, SemanticState::Off),
            EmbedderMode::Local => {
                if !local::is_installed(&self.env.models) {
                    (None, SemanticState::ModelMissing)
                } else if let Some(w) = self.env.worker() {
                    let e: SharedEmbedder = Arc::new(LocalEmbedder::new(
                        w,
                        local::model_path(&self.env.models),
                        local::INDEX_DIM,
                    ));
                    (Some(e), SemanticState::Off)
                } else {
                    (
                        None,
                        SemanticState::Failed {
                            error: AppError::new(ErrorCode::EmbedderMissing).with_detail("lowk-embed-worker not found"),
                        },
                    )
                }
            }
            EmbedderMode::Cloud => match self.env.keys.openrouter() {
                Some(k) => {
                    let e: SharedEmbedder = Arc::new(CloudEmbedder::new(
                        &s.llm.base_url,
                        &k,
                        &s.embedder.cloud_model,
                        local::INDEX_DIM,
                    ));
                    (Some(e), SemanticState::Off)
                }
                None => (
                    None,
                    SemanticState::Failed {
                        error: AppError::new(ErrorCode::ApiKeyMissing),
                    },
                ),
            },
        };
        let warm = emb.clone();
        if let Err(e) = self.index.set_embedder(emb, state) {
            tracing::warn!(?e, "embedder not applied");
        }
        // Warm-up: the sidecar loads its model now, so the first search / Ask does not pay for it.
        if let Some(w) = warm.filter(|w| !w.is_cloud()) {
            std::thread::Builder::new()
                .name("lowk-embed-warm".into())
                .spawn(move || {
                    let t0 = std::time::Instant::now();
                    match w.embed_query(&w.format_query("warm up")) {
                        Ok(_) => tracing::info!(ms = t0.elapsed().as_millis() as u64, "embedder warm"),
                        Err(e) => tracing::warn!(?e, "embedder warm-up failed"),
                    }
                })
                .ok();
        }
        self.emit_status();
    }

    pub fn embedder_info(&self) -> EmbedderInfo {
        let installing = self.embed.cancel.lock().is_some();
        gemma_info(local::is_installed(&self.env.models), installing)
    }

    /// Downloads the model (SHA-256 pinned, resumable); returns when installed, failed or cancelled.
    pub fn embedder_install(&self) -> Result<()> {
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut g = self.embed.cancel.lock();
            if g.is_some() {
                return Err(AppError::new(ErrorCode::AlreadyExists).with_detail("embedder download running"));
            }
            *g = Some(cancel.clone());
        }
        let sink = self.env.sink.clone();
        let r = local::install(&self.env.models, &cancel, &mut |p| {
            sink.emit(AppEvent::EngineProgress(p));
        });
        *self.embed.cancel.lock() = None;
        r?;
        // the install button is the user's consent to the local embedder
        if self.settings().embedder.mode == EmbedderMode::Off {
            let _ = self.env.settings.patch(SettingsPatch(serde_json::json!({ "embedder": { "mode": "local" } })));
        }
        self.apply_embedder();
        Ok(())
    }

    pub fn embedder_install_cancel(&self) {
        if let Some(c) = self.embed.cancel.lock().as_ref() {
            c.store(true, Ordering::SeqCst);
        }
    }
}

#[allow(dead_code)]
const _: &str = GEMMA_ENGINE_ID;
