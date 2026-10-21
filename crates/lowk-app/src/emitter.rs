//! `EventBus` -> Tauri events (`emit_to` per window). Partial STT updates are
//! coalesced to <= 20 Hz per utterance; everything else is forwarded as is.
use lowk_types::{AppEvent, EventSink, OverlayEvent, Stream, SttEvent, SttKind, UttId};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, EventTarget};

const OVERLAY: &str = "overlay";
const MAIN: &str = "main";
const PILL: &str = "pill";

type StoreTap = Box<dyn Fn(&lowk_types::StoreChange) + Send + Sync>;

pub struct UiEmitter {
    app: AppHandle,
    /// Latest unsent partial per utterance. The lock is held while sending so a
    /// stale partial can never overtake the final of the same utterance.
    pending: Mutex<HashMap<(Stream, UttId), SttEvent>>,
    /// Every store change (ours and the watcher's) also reaches the index feed.
    store_tap: std::sync::OnceLock<StoreTap>,
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
        AppEvent::SttLoading(l) => json!({ "loading": l }),
        AppEvent::EngineProgress(e) => v(serde_json::to_value(e)),
        AppEvent::AudioNotice(k) => json!({ "kind": k }),
        AppEvent::Ingest(e) => v(serde_json::to_value(e)),
        AppEvent::Index(e) => v(serde_json::to_value(e)),
        AppEvent::Summary(e) => v(serde_json::to_value(e)),
    }
}

/// `overlay://*` never reaches the main window; the main window's own
/// diagnostics never need the pill.
fn targets(ev: &AppEvent) -> &'static [&'static str] {
    match ev {
        AppEvent::Overlay(_) => &[OVERLAY],
        AppEvent::Session(_) | AppEvent::Stealth(_) => &[OVERLAY, MAIN, PILL],
        AppEvent::Bind(_) | AppEvent::Store(_) => &[MAIN],
        AppEvent::Ingest(_) | AppEvent::Index(_) | AppEvent::Summary(_) => &[MAIN],
        _ => &[OVERLAY, MAIN],
    }
}

/// Matches the webview windows named in `labels` (and `Any` listeners, which
/// Tauri always includes).
fn label_filter<'a>(labels: &'a [&'a str]) -> impl Fn(&EventTarget) -> bool + 'a {
    move |t| match t {
        EventTarget::WebviewWindow { label }
        | EventTarget::Webview { label }
        | EventTarget::Window { label }
        | EventTarget::AnyLabel { label } => labels.contains(&label.as_str()),
        _ => false,
    }
}

impl UiEmitter {
    pub fn new(app: AppHandle) -> Arc<Self> {
        let e = Arc::new(Self {
            app,
            pending: Mutex::new(HashMap::new()),
            store_tap: std::sync::OnceLock::new(),
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

    /// ONE `emit_filter` call per event. Never loop `emit_to` over several labels:
    /// a JS `listen()` registers with target `Any`, and Tauri delivers an event
    /// to every `Any` listener of the webview for *each* `emit_to` call, whatever
    /// its label. Two labels = every handler runs twice (F10: duplicated deltas).
    fn send(&self, name: &str, targets: &[&str], p: &Value) {
        let _ = self.app.emit_filter(name, p.clone(), label_filter(targets));
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

    /// A plain named event to the main window only.
    pub fn to_main(&self, name: &str, p: Value) {
        self.send(name, &[MAIN], &p);
    }

    pub fn set_store_tap(&self, f: StoreTap) {
        let _ = self.store_tap.set(f);
    }

    /// Emit a plain named event to all windows (e.g. `settings://changed`).
    pub fn broadcast(&self, name: &str, p: Value) {
        self.send(name, &[OVERLAY, MAIN, PILL], &p);
    }
}

impl EventSink for UiEmitter {
    fn emit(&self, ev: AppEvent) {
        if let AppEvent::Store(c) = &ev {
            if let Some(t) = self.store_tap.get() {
                t(c);
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_only_named_labels() {
        let f = label_filter(&[OVERLAY, MAIN]);
        assert!(f(&EventTarget::webview_window("overlay")));
        assert!(f(&EventTarget::webview_window("main")));
        assert!(!f(&EventTarget::webview_window("pill")));
        assert!(!f(&EventTarget::Any));
    }

    /// F10 regression guard: delivery is a single call, so the emitter source must
    /// never loop `emit_to` over targets again.
    #[test]
    fn send_is_a_single_emit_call() {
        let src = include_str!("emitter.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(
            !code.contains(".emit_to("),
            "loop of emit_to duplicates deliveries to Any listeners"
        );
    }
}
