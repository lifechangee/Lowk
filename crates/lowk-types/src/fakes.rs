//! Tiny `Fake*` / `Noop*` implementations of the contract traits so dependents
//! can build and test before the real owners land.
use crate::*;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Runs everything inline on the calling thread.
#[derive(Default, Clone, Copy)]
pub struct InlineMainThread;
impl MainThread for InlineMainThread {
    fn run(&self, f: Box<dyn FnOnce() + Send>) {
        f()
    }
    fn is_main(&self) -> bool {
        true
    }
}

#[derive(Default, Clone, Copy)]
pub struct FixedCursor(pub f64, pub f64);
impl CursorSource for FixedCursor {
    fn position(&self) -> (f64, f64) {
        (self.0, self.1)
    }
}

#[derive(Default, Clone, Copy)]
pub struct NoopRetriever;
impl Retriever for NoopRetriever {
    fn retrieve(&self, _q: &RetrievalQuery) -> Result<Vec<ContextChunk>> {
        Ok(vec![])
    }
}

/// In-memory overlay controller that records mode/visibility.
pub struct NoopOverlayController {
    state: Mutex<(bool, OverlayMode)>,
}
impl Default for NoopOverlayController {
    fn default() -> Self {
        Self {
            state: Mutex::new((false, OverlayMode::Passive)),
        }
    }
}
impl NoopOverlayController {
    pub fn visible(&self) -> bool {
        self.state.lock().unwrap().0
    }
}
impl OverlayController for NoopOverlayController {
    fn show(&self) -> Result<()> {
        self.state.lock().unwrap().0 = true;
        Ok(())
    }
    fn hide(&self) {
        self.state.lock().unwrap().0 = false;
    }
    fn panic_hide(&self) {
        self.hide()
    }
    fn set_mode(&self, m: OverlayMode) -> Result<()> {
        self.state.lock().unwrap().1 = m;
        Ok(())
    }
    fn mode(&self) -> OverlayMode {
        self.state.lock().unwrap().1
    }
    fn move_hold(&self, _phase: Phase, _cursor: &dyn CursorSource) {}
    fn status(&self) -> StealthStatus {
        StealthStatus::Unavailable("noop controller".into())
    }
    fn selftest(&self) -> Result<SelfTestReport> {
        Err(AppError::not_implemented())
    }
}

/// In-memory settings store.
pub struct MemSettingsStore {
    cur: Mutex<Settings>,
    subs: Mutex<Vec<Sender<Settings>>>,
}
impl MemSettingsStore {
    pub fn new(s: Settings) -> Self {
        Self {
            cur: Mutex::new(s),
            subs: Mutex::new(vec![]),
        }
    }
}
impl Default for MemSettingsStore {
    fn default() -> Self {
        Self::new(Settings::default())
    }
}
impl SettingsStore for MemSettingsStore {
    fn get(&self) -> Settings {
        self.cur.lock().unwrap().clone()
    }
    fn patch(&self, p: SettingsPatch) -> Result<Settings> {
        let mut cur = self.cur.lock().unwrap();
        let next = cur.apply_patch(&p)?;
        *cur = next.clone();
        self.subs.lock().unwrap().retain(|s| s.send(next.clone()).is_ok());
        Ok(next)
    }
    fn subscribe(&self) -> Receiver<Settings> {
        let (tx, rx) = channel();
        self.subs.lock().unwrap().push(tx);
        rx
    }
}

/// Collects STT events and errors (tests).
#[derive(Default)]
pub struct CollectSttSink {
    pub events: Mutex<Vec<SttEvent>>,
    pub errors: Mutex<Vec<SttError>>,
}
impl SttSink for CollectSttSink {
    fn emit(&self, ev: SttEvent) {
        self.events.lock().unwrap().push(ev)
    }
    fn error(&self, e: SttError) {
        self.errors.lock().unwrap().push(e)
    }
}

/// Collects bus events (tests).
#[derive(Default)]
pub struct CollectEventSink(pub Mutex<Vec<AppEvent>>);
impl EventSink for CollectEventSink {
    fn emit(&self, ev: AppEvent) {
        self.0.lock().unwrap().push(ev)
    }
}

pub fn arc_sink() -> Arc<CollectSttSink> {
    Arc::new(CollectSttSink::default())
}
