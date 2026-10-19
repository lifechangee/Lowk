//! Session orchestrator: state machine, event bus, transcript buffer, demo
//! feed. M0 stub (WP7 owns it). Wires feature crates only through traits.
use lowk_types::*;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

pub struct SessionDeps {
    pub main_thread: Arc<dyn MainThread>,
    pub retriever: Arc<dyn Retriever>,
    pub settings: Arc<dyn SettingsStore>,
}

pub struct Session {
    _deps: SessionDeps,
    state: Mutex<SessionState>,
    subs: Mutex<Vec<Sender<AppEvent>>>,
}

impl Session {
    pub fn new(deps: SessionDeps) -> Self {
        Self {
            _deps: deps,
            state: Mutex::new(SessionState {
                phase: SessionPhase::Idle,
                call: None,
                demo: false,
            }),
            subs: Mutex::new(vec![]),
        }
    }
    pub fn start(&self, _o: StartOpts) -> Result<CallId> {
        Err(AppError::not_implemented())
    }
    pub fn stop(&self) -> Result<()> {
        Err(AppError::new(ErrorCode::SessionIdle))
    }
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            state: self.state.lock().unwrap().clone(),
            clock: None,
            transcript: vec![],
            queue: QueueState::default(),
        }
    }
    pub fn subscribe(&self) -> Receiver<AppEvent> {
        let (tx, rx) = channel();
        self.subs.lock().unwrap().push(tx);
        rx
    }
}
