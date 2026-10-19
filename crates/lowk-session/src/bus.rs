//! Low-rate event bus. Subscribers get their own unbounded channel (a slow
//! consumer never blocks the producer; it resyncs from `Session::snapshot`).
use lowk_types::{AppEvent, EventSink};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct EventBus {
    subs: Mutex<Vec<Sender<AppEvent>>>,
    sinks: Mutex<Vec<Arc<dyn EventSink>>>,
}

impl EventBus {
    pub fn subscribe(&self) -> Receiver<AppEvent> {
        let (tx, rx) = channel();
        self.subs.lock().unwrap().push(tx);
        rx
    }

    /// Callback-style consumer (the `UiEmitter` in `lowk-app`). Called on the
    /// emitting thread, so it must not block or call back into the session.
    pub fn add_sink(&self, sink: Arc<dyn EventSink>) {
        self.sinks.lock().unwrap().push(sink);
    }

    pub fn emit(&self, ev: AppEvent) {
        self.subs.lock().unwrap().retain(|s| s.send(ev.clone()).is_ok());
        let sinks: Vec<_> = self.sinks.lock().unwrap().clone();
        for s in sinks {
            s.emit(ev.clone());
        }
    }

    pub fn emit_all(&self, evs: impl IntoIterator<Item = AppEvent>) {
        for e in evs {
            self.emit(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::fakes::CollectEventSink;
    use lowk_types::{AudioLevel, QueueState};

    #[test]
    fn fans_out_and_drops_dead_subscribers() {
        let bus = EventBus::default();
        let a = bus.subscribe();
        let b = bus.subscribe();
        let sink = Arc::new(CollectEventSink::default());
        bus.add_sink(sink.clone());
        drop(b);
        bus.emit(AppEvent::Queue(QueueState::default()));
        bus.emit(AppEvent::Level(AudioLevel { me: 0.1, sys: 0.2 }));
        assert_eq!(a.try_iter().count(), 2);
        assert_eq!(sink.0.lock().unwrap().len(), 2);
        assert_eq!(bus.subs.lock().unwrap().len(), 1);
    }
}
