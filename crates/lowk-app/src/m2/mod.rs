//! M2 integration: documents, ingest queue, index feed, embedder, search, summary/tags and the
//! Ask flow's LLM side. Thin glue over `lowk-store`, `lowk-ingest`, `lowk-index` and `lowk-llm`;
//! the logic stays in those crates. Nothing here knows Tauri: `Services` talks to the UI through
//! an [`EventSink`] and to settings through [`SettingsStore`], so the same code runs in the app,
//! in `cargo test` and in the bundle's headless self-check (`selfcheck.rs`).
pub mod embed;
pub mod feed;
pub mod hints;
pub mod ingest;
pub mod retx;
pub mod route;
pub mod selfcheck;
pub mod summary;

use lowk_index::embedder::models::ModelStore;
use lowk_index::Index;
use lowk_store::Vault;
use lowk_types::*;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

/// Where the BYOK key comes from. The app reads the OS keychain; tests inject a value.
pub trait KeyProvider: Send + Sync {
    fn openrouter(&self) -> Option<String>;
}

/// Keychain (`openrouter`), plus the `OPENROUTER_API_KEY` environment variable in dev builds or when
/// `LOWK_DEV_KEYS` is set (same rule as the cloud STT keys, `lowk_stt::cloud::resolve_key`).
pub struct SystemKeys;
impl KeyProvider for SystemKeys {
    fn openrouter(&self) -> Option<String> {
        if cfg!(debug_assertions) || std::env::var_os("LOWK_DEV_KEYS").is_some() {
            if let Ok(k) = std::env::var("OPENROUTER_API_KEY") {
                if !k.trim().is_empty() {
                    return Some(k);
                }
            }
        }
        lowk_platform::secrets::get("openrouter")
            .ok()
            .filter(|k| !k.trim().is_empty())
    }
}

/// Fixed key (tests, self-check).
pub struct StaticKey(pub Option<String>);
impl KeyProvider for StaticKey {
    fn openrouter(&self) -> Option<String> {
        self.0.clone()
    }
}

/// Everything that is not per vault.
#[derive(Clone)]
pub struct Env {
    pub settings: Arc<dyn SettingsStore>,
    pub sink: Arc<dyn EventSink>,
    pub keys: Arc<dyn KeyProvider>,
    pub models: ModelStore,
    /// `lowk-embed-worker` path; `None` = search for it next to the executable (bundle) and in dev locations.
    pub embed_worker: Option<PathBuf>,
}

impl Env {
    pub fn emit(&self, ev: AppEvent) {
        self.sink.emit(ev);
    }
    pub fn worker(&self) -> Option<PathBuf> {
        self.embed_worker
            .clone()
            .or_else(lowk_index::embedder::find_worker)
    }
}

pub(crate) enum Job {
    Reconcile,
    Item(Ulid),
    Folder(Ulid),
}

/// Per-vault services: the index (`<vault>/.lowk/index.db`, rebuildable), the feed that keeps it
/// in step with the vault, the ingest queue and the embedder.
pub struct Services {
    pub env: Env,
    pub vault: Vault,
    pub index: Index,
    pub(crate) feed: Sender<Job>,
    pub(crate) ingest: ingest::Queue,
    pub(crate) embed: embed::EmbedState,
    pub(crate) summaries: Mutex<HashMap<CallId, Arc<AtomicBool>>>,
    pub(crate) retx: retx::RetxState,
}

/// The index file of a vault.
pub fn index_path(vault: &Vault) -> PathBuf {
    vault.root().join(lowk_store::vault::SERVICE_DIR).join("index.db")
}

/// Opens the index; a corrupt or foreign file is rebuilt from the vault (it is only a projection).
fn open_index(vault: &Vault) -> Result<Index> {
    let p = index_path(vault);
    match Index::open(&p) {
        Ok(i) => Ok(i),
        Err(e) => {
            tracing::warn!(?e, "index.db unreadable; rebuilding it from the vault");
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", p.display()));
            }
            Index::open(&p)
        }
    }
}

impl Services {
    /// Opens the index, starts the feed (initial reconcile), the ingest workers (re-queues documents
    /// a crash left unfinished) and the embedder per settings.
    pub fn start(env: Env, vault: Vault) -> Result<Arc<Self>> {
        let index = open_index(&vault)?;
        let sink = env.sink.clone();
        index.on_status(Box::new(move |s| sink.emit(AppEvent::Index(s.clone()))));
        let (feed_tx, feed_rx) = std::sync::mpsc::channel::<Job>();
        let (ingest_q, ingest_rx) = ingest::Queue::new();
        let svc = Arc::new_cyclic(|weak| {
            feed::spawn(weak.clone(), feed_rx);
            ingest::spawn_workers(weak.clone(), ingest_rx);
            Services {
                env,
                vault,
                index,
                feed: feed_tx,
                ingest: ingest_q,
                embed: embed::EmbedState::default(),
                summaries: Mutex::new(HashMap::new()),
                retx: retx::RetxState::default(),
            }
        });
        svc.index.start_background();
        let _ = svc.feed.send(Job::Reconcile);
        svc.recover_ingest();
        svc.apply_embedder();
        Ok(svc)
    }

    pub fn settings(&self) -> Settings {
        self.env.settings.get()
    }

    /// A store change (ours or the watcher's): keep the index in step, start queued documents.
    pub fn on_store_change(&self, c: &StoreChange) {
        let _ = self.feed.send(if c.is_folder {
            Job::Folder(c.id)
        } else {
            Job::Item(c.id)
        });
        if !c.is_folder
            && matches!(
                c.kind,
                StoreChangeKind::Created | StoreChangeKind::Updated | StoreChangeKind::Restored
            )
        {
            if let Ok(d) = self.vault.get_doc(&c.id) {
                if d.status == IngestStatus::Queued && !d.deleted {
                    self.ingest.enqueue(c.id);
                }
            }
        }
    }

    /// Settings that act at once: the embedder (mode, model) is re-applied.
    pub fn on_settings(&self, old: &Settings, new: &Settings) {
        if old.embedder != new.embedder {
            self.apply_embedder();
        }
    }

    /// Full diff of the vault against the index (after imports, which the watcher does not report).
    pub fn request_reconcile(&self) {
        let _ = self.feed.send(Job::Reconcile);
    }

    pub fn emit_status(&self) {
        use lowk_types::SearchIndex as _;
        self.env.emit(AppEvent::Index(self.index.status()));
    }
}

/// `Retriever` over whatever index the open vault has (none while no vault is open: no context).
pub struct DynRetriever(pub Arc<RwLock<Option<Index>>>);
impl Retriever for DynRetriever {
    fn retrieve(&self, q: &RetrievalQuery) -> Result<Vec<ContextChunk>> {
        match self.0.read().as_ref() {
            Some(i) => i.retrieve(q),
            None => Ok(vec![]),
        }
    }
    fn retrieve_pack(&self, q: &RetrievalQuery) -> Result<ContextPack> {
        match self.0.read().as_ref() {
            Some(i) => i.retrieve_pack(q),
            None => Ok(ContextPack::default()),
        }
    }
}

/// Folder facts for policy checks.
pub fn folder_allows_cloud(vault: &Vault, folder: &FolderRef) -> bool {
    match folder {
        FolderRef::User(_) => vault
            .folders()
            .ok()
            .and_then(|fs| fs.into_iter().find(|f| f.id == *folder))
            // an unknown / unreadable folder is treated as private: policy fails closed
            .is_some_and(|f| f.allow_cloud),
        FolderRef::Unsorted => true,
        FolderRef::Trash => false,
    }
}
