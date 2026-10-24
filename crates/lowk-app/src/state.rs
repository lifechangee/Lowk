//! Shared application state managed by Tauri (`State<'_, AppState>`).
use crate::emitter::UiEmitter;
use lowk_input::InputManager;
use lowk_session::Session;
use lowk_stealth::{SamplerSlot, Stealth};
use lowk_store::{FileSettingsStore, Vault};
use lowk_types::*;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::AppHandle;

type Job = Box<dyn FnOnce() + Send>;

/// One worker thread that runs UI-side effects (window show/hide, visibility)
/// strictly in order, off every event-producing thread. This keeps session and
/// input threads from ever blocking on the main thread while holding locks.
pub struct Driver {
    tx: Sender<Job>,
}

impl Driver {
    pub fn new() -> Self {
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("lowk-driver".into())
            .spawn(move || {
                for job in rx {
                    job();
                }
            })
            .expect("spawn driver");
        Self { tx }
    }
    pub fn run(&self, f: impl FnOnce() + Send + 'static) {
        let _ = self.tx.send(Box::new(f));
    }
}

pub type Shared = Arc<AppState>;

pub struct AppState {
    pub app: AppHandle,
    pub settings: Arc<FileSettingsStore>,
    pub vault: Arc<RwLock<Option<Vault>>>,
    pub session: Arc<Session>,
    pub input: Mutex<InputManager>,
    pub emitter: Arc<UiEmitter>,
    pub driver: Driver,
    pub stealth: OnceLock<Arc<Stealth>>,
    /// The adaptive-scrim sampler; empty (and never constructed) while the setting is off.
    pub tone: SamplerSlot,
    pub binds_report: Mutex<BindReport>,
    pub applied_binds: Mutex<Vec<BindSpec>>,
    pub drag_origin: Mutex<Option<(f64, f64)>>,
    /// G2: pointer-drag origin of the pill window.
    pub pill_drag_origin: Mutex<Option<(f64, f64)>>,
    /// G6: generation counter that debounces the click-hold key (a quick Ctrl+C is not a hold).
    /// 1 while the ⌥ magnet moves the pill (so its release is routed to the pill).
    pub pill_moving: std::sync::atomic::AtomicU8,
    pub hover_gen: std::sync::atomic::AtomicU64,
    /// Template chosen in the overlay chip (per run, not persisted).
    pub template: Mutex<String>,
    /// The user hid the overlay by hand (ToggleOverlay) during this recording.
    pub overlay_user_hidden: AtomicBool,
    /// Speech engines (local sidecar + cloud) and the running model download, if any.
    pub engines: Arc<lowk_stt::EngineRegistry>,
    pub install_cancel: Mutex<Option<Arc<AtomicBool>>>,
    /// G7: screenshots waiting for the next Ask (in memory only).
    pub shots: Arc<crate::commands::capture::ShotQueue>,
    /// M2 environment (settings, event sink, keys, model store) and the per-vault services.
    pub env: crate::m2::Env,
    pub m2: RwLock<Option<Arc<crate::m2::Services>>>,
    /// The open vault's index, shared with the session's retriever.
    pub index_slot: Arc<RwLock<Option<lowk_index::Index>>>,
    watcher_stop: Mutex<Option<Arc<AtomicBool>>>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app: AppHandle,
        settings: Arc<FileSettingsStore>,
        vault: Arc<RwLock<Option<Vault>>>,
        session: Arc<Session>,
        input: InputManager,
        emitter: Arc<UiEmitter>,
        engines: Arc<lowk_stt::EngineRegistry>,
        env: crate::m2::Env,
        index_slot: Arc<RwLock<Option<lowk_index::Index>>>,
        shots: Arc<crate::commands::capture::ShotQueue>,
    ) -> Self {
        let template = settings.get().llm.default_template;
        Self {
            app,
            settings,
            vault,
            session,
            input: Mutex::new(input),
            emitter,
            driver: Driver::new(),
            stealth: OnceLock::new(),
            tone: SamplerSlot::default(),
            binds_report: Mutex::new(BindReport::default()),
            applied_binds: Mutex::new(vec![]),
            drag_origin: Mutex::new(None),
            pill_drag_origin: Mutex::new(None),
            pill_moving: std::sync::atomic::AtomicU8::new(0),
            hover_gen: std::sync::atomic::AtomicU64::new(0),
            template: Mutex::new(template),
            overlay_user_hidden: AtomicBool::new(false),
            engines,
            install_cancel: Mutex::new(None),
            shots,
            env,
            m2: RwLock::new(None),
            index_slot,
            watcher_stop: Mutex::new(None),
        }
    }

    pub fn stealth(&self) -> Result<&Arc<Stealth>> {
        self.stealth.get().ok_or_else(|| {
            AppError::new(ErrorCode::StealthUnavailable).with_detail("stealth not initialised")
        })
    }

    pub fn vault(&self) -> Result<Vault> {
        self.vault
            .read()
            .clone()
            .ok_or_else(|| AppError::new(ErrorCode::VaultNotOpen))
    }

    /// Pre-connects what the first Ask needs, in the background: the LLM provider connection (TLS)
    /// and the local query embedder. Sends no user data. Called at app start, when a recording
    /// starts and when a new key is saved.
    pub fn spawn_ask_warmup(&self, folder: Option<FolderRef>) {
        if std::env::var_os("LOWK_NO_WARMUP").is_some() {
            return;
        }
        let session = self.session.clone();
        let idx = self.m2.read().as_ref().map(|s| s.index.clone());
        let _ = std::thread::Builder::new()
            .name("lowk-ask-warm".into())
            .spawn(move || {
                session.warm_hints(&folder.unwrap_or(FolderRef::Unsorted));
                if let Some(ix) = idx {
                    ix.warm_query_embedder();
                }
            });
    }

    /// The services of the open vault (documents, index, summary); opens the configured vault on first use.
    pub fn svc(&self) -> Result<Arc<crate::m2::Services>> {
        if let Some(s) = self.m2.read().clone() {
            return Ok(s);
        }
        self.open_vault(None)?;
        self.m2
            .read()
            .clone()
            .ok_or_else(|| AppError::new(ErrorCode::VaultNotOpen))
    }

    pub fn emit(&self, ev: AppEvent) {
        use lowk_types::EventSink;
        self.emitter.emit(ev);
    }

    /// Open the vault at `path` (default: `settings.vault.path`) and start
    /// watching it. Persists a non-default path back to the settings.
    pub fn open_vault(&self, path: Option<String>) -> Result<VaultInfo> {
        let want = path.unwrap_or_else(|| self.settings.get().vault.path);
        let dir = resolve_vault_path(&want)?;
        let vault = Vault::open(&dir)?;
        let info = VaultInfo {
            path: vault.root().display().to_string(),
        };
        if let Some(stop) = self.watcher_stop.lock().take() {
            stop.store(true, Ordering::SeqCst);
        }
        self.start_watcher(&vault);
        *self.vault.write() = Some(vault.clone());
        match crate::m2::Services::start(self.env.clone(), vault) {
            Ok(svc) => {
                *self.index_slot.write() = Some(svc.index.clone());
                svc.spawn_provider_probe();
                *self.m2.write() = Some(svc);
            }
            Err(e) => {
                // never a silent fallback: search and documents report this error
                tracing::error!(?e, "index services could not start");
                *self.index_slot.write() = None;
                *self.m2.write() = None;
            }
        }
        if want != self.settings.get().vault.path {
            let _ = self
                .settings
                .patch(SettingsPatch(serde_json::json!({ "vault": { "path": want } })));
        }
        Ok(info)
    }

    fn start_watcher(&self, vault: &Vault) {
        let Ok(watcher) = vault.watch(Duration::from_millis(1500)) else {
            tracing::warn!("vault watcher could not start");
            return;
        };
        let stop = Arc::new(AtomicBool::new(false));
        *self.watcher_stop.lock() = Some(stop.clone());
        let emitter = self.emitter.clone();
        std::thread::Builder::new()
            .name("lowk-vault-watch".into())
            .spawn(move || {
                use lowk_types::EventSink;
                while !stop.load(Ordering::SeqCst) {
                    if let Ok(batch) = watcher.changes().recv_timeout(Duration::from_millis(400)) {
                        for c in batch {
                            emitter.emit(AppEvent::Store(c));
                        }
                    }
                }
            })
            .expect("spawn vault watcher");
    }
}

/// Settings store the commands see (read-only filtered view for the overlay is
/// not needed: the overlay capability only grants `settings_get`).
pub fn data_dir() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("LOWK_DATA_DIR") {
        return Ok(PathBuf::from(p));
    }
    lowk_platform::dirs::app_data().ok_or_else(|| AppError::new(ErrorCode::Io).with_detail("no app data dir"))
}

pub fn resolve_vault_path(p: &str) -> Result<PathBuf> {
    let bad = || AppError::new(ErrorCode::VaultInvalid).with_detail(format!("cannot resolve {p}"));
    if p == DEFAULT_VAULT_PATH {
        return lowk_platform::dirs::default_vault().ok_or_else(bad);
    }
    if let Some(rest) = p.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(bad)?;
        return Ok(PathBuf::from(home).join(rest));
    }
    let path = PathBuf::from(p);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(bad())
    }
}
