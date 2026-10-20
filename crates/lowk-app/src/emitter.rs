//! `EventBus` -> Tauri events (`emit_to` per window). Partial STT updates are
//! coalesced to <= 20 Hz per utterance; everything else is forwarded as is.
use lowk_types::{AppEvent, EventSink, OverlayEvent, Stream, SttEvent, SttKind, UttId};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const OVERLAY: &str = "overlay";
const MAIN: &str = "main";
const PILL: &str = "pill";

pub struct UiEmitter {
    app: AppHandle,
    /// Latest unsent partial per utterance. The lock is held while sending so a
    /// stale partial can never overtake the final of the same utterance.
    pending: Mutex<HashMap<(Stream, UttId), SttEvent>>,
}

fn payload(ev: &AppEvent) -> Value {
    let v = |r: serde_json::Result<Value>| r.unwrap_or(Value::Null);
    match ev {
        AppEvent::Stt(e) => v(serde_json::to_value(e)),
        AppEvent::Session(e) => v(serde_json::to_value(e)),
        AppEvent::Llm(e) => v(serde_json::to_value(e)),
        AppEvent::Overlay(OverlayEvent::Mode(m)) => json!({ "mode": m }),
        AppEvent::Overlay(OverlayEvent::Tone(t)) => v(serde_json::to_value(t)),
        AppEvent::Stealth(e) => v(serde_json::to_value(e)),
        AppEvent::Bind(e) => v(serde_json::to_value(e)),
        AppEvent::Perm { kind, state } => json!({ "kind": kind, "state": state }),
        AppEvent::Queue(e) => v(serde_json::to_value(e)),
        AppEvent::Store(e) => v(serde_json::to_value(e)),
        AppEvent::Level(e) => v(serde_json::to_value(e)),
        AppEvent::MyAnswer(e) => v(serde_json::to_value(e)),
        AppEvent::SttError(e) => v(serde_json::to_value(e)),
    }
}

/// `overlay://*` never reaches the main window; the main window's own
/// diagnostics never need the pill.
fn targets(ev: &AppEvent) -> &'static [&'static str] {
    match ev {
        AppEvent::Overlay(_) => &[OVERLAY],
        AppEvent::Session(_) | AppEvent::Stealth(_) => &[OVERLAY, MAIN, PILL],
        AppEvent::Bind(_) | AppEvent::Store(_) => &[MAIN],
        _ => &[OVERLAY, MAIN],
    }
}

impl UiEmitter {
    pub fn new(app: AppHandle) -> Arc<Self> {
        let e = Arc::new(Self {
            app,
            pending: Mutex::new(HashMap::new()),
        });
        let weak = Arc::downgrade(&e);
        std::thread::Builder::new()
            .name("lowk-emit".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(50));
                let Some(e) = weak.upgrade() else { return };
                e.flush_partials();
            })
            .expect("spawn emitter");
        e
    }

    fn send(&self, name: &str, targets: &[&str], p: &Value) {
        for t in targets {
            let _ = self.app.emit_to(*t, name, p.clone());
        }
    }

    fn flush_partials(&self) {
        let mut g = self.pending.lock();
        if g.is_empty() {
            return;
        }
        for (_, e) in g.drain() {
            let ev = AppEvent::Stt(e);
            self.send(ev.name(), targets(&ev), &payload(&ev));
        }
    }

    /// Emit a plain named event to all windows (e.g. `settings://changed`).
    pub fn broadcast(&self, name: &str, p: Value) {
        self.send(name, &[OVERLAY, MAIN, PILL], &p);
    }
}

impl EventSink for UiEmitter {
    fn emit(&self, ev: AppEvent) {
        if let AppEvent::Stt(e) = &ev {
            let key = (e.stream, e.utt);
            let mut g = self.pending.lock();
            if e.kind == SttKind::Partial {
                g.insert(key, e.clone());
                return;
            }
            g.remove(&key);
            self.send(ev.name(), targets(&ev), &payload(&ev));
            return;
        }
        self.send(ev.name(), targets(&ev), &payload(&ev));
    }
}
