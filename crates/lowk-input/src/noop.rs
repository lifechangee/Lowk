//! Bind model backends: combo (plugin adapter), low-level kb/mouse, cursor
//! source, conflict detection. Stub.
use lowk_types::*;
use std::sync::mpsc::{channel, Receiver, Sender};

/// Accepts binds, reports them active, never emits events.
pub struct NoopInputBackend {
    tx: Sender<BindEvent>,
    rx: std::sync::Mutex<Option<Receiver<BindEvent>>>,
}
impl Default for NoopInputBackend {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx: std::sync::Mutex::new(Some(rx)),
        }
    }
}
impl NoopInputBackend {
    /// Test helper: inject an event as if a bind fired.
    pub fn inject(&self, ev: BindEvent) {
        let _ = self.tx.send(ev);
    }
}
impl InputBackend for NoopInputBackend {
    fn set_binds(&mut self, binds: &[BindSpec]) -> Result<BindReport> {
        Ok(BindReport {
            entries: binds
                .iter()
                .map(|b| BindStatusEntry {
                    action: b.action,
                    template: b.template.clone(),
                    status: BindStatus::Active,
                })
                .collect(),
        })
    }
    fn events(&self) -> Receiver<BindEvent> {
        // Single-consumer channel: the first call hands out the receiver.
        self.rx.lock().unwrap().take().unwrap_or_else(|| channel().1)
    }
}
