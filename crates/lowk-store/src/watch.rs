//! Vault watcher: `notify` events are debounced, then reconciled with a scan
//! (`Vault::rescan`), so editors that write via temp+rename, sync clients and
//! post-sleep gaps all end in the same place. Our own mutations never report.
use crate::util::err;
use crate::vault::{Vault, SERVICE_DIR};
use lowk_types::*;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::Duration;

/// Keep alive to keep watching. Dropping stops the thread.
pub struct VaultWatcher {
    _w: RecommendedWatcher,
    rx: Receiver<Vec<StoreChange>>,
}

impl VaultWatcher {
    /// Batches of changes, one per debounce window.
    pub fn changes(&self) -> &Receiver<Vec<StoreChange>> {
        &self.rx
    }
}

impl Vault {
    /// Watch the vault root. `debounce` is the quiet period before reconciling
    /// (architecture: 1-2 s in the app; tests use less).
    pub fn watch(&self, debounce: Duration) -> Result<VaultWatcher> {
        let (ev_tx, ev_rx) = channel::<()>();
        let svc = self.root().join(SERVICE_DIR);
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(e) = res {
                if e.paths.iter().any(|p| !p.starts_with(&svc)) {
                    let _ = ev_tx.send(());
                }
            }
        })
        .map_err(|e| err(ErrorCode::Io, e.to_string()))?;
        w.watch(self.root(), RecursiveMode::Recursive)
            .map_err(|e| err(ErrorCode::Io, e.to_string()))?;
        let (tx, rx) = channel();
        let vault = self.clone();
        std::thread::Builder::new()
            .name("lowk-vault-watch".into())
            .spawn(move || loop {
                if ev_rx.recv().is_err() {
                    return;
                }
                // wait for a quiet period
                loop {
                    match ev_rx.recv_timeout(debounce) {
                        Ok(()) => continue,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                match vault.rescan() {
                    Ok(c) if !c.is_empty() => {
                        if tx.send(c).is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("vault rescan failed: {e}"),
                }
            })
            .map_err(|e| err(ErrorCode::Io, e.to_string()))?;
        Ok(VaultWatcher { _w: w, rx })
    }
}
