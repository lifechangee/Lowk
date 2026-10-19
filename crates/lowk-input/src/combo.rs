//! Combo backend adapter. `lowk-input` cannot depend on Tauri, so the app shell
//! implements [`ComboRegistrar`] over `tauri-plugin-global-shortcut` (Carbon
//! `RegisterEventHotKey` / `RegisterHotKey`, no permission) and feeds its
//! pressed/released callbacks into [`ComboEvents`].
use lowk_types::{
    now_mono_ns, ActionId, BindEvent, BindReport, BindSpec, BindStatus, BindStatusEntry, InputBackend,
    KeyCode, Mods, Phase, Result, Trigger,
};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

/// Implemented by `lowk-app` over the Tauri global-shortcut plugin.
pub trait ComboRegistrar: Send {
    /// Register one plain combo. Err = the OS or another app already owns it.
    fn register(&mut self, action: ActionId, mods: Mods, key: &KeyCode) -> std::result::Result<(), String>;
    /// Drop every registration made through this registrar.
    fn unregister_all(&mut self);
}

/// Clonable handle the shell calls from its shortcut handler.
#[derive(Clone)]
pub struct ComboEvents {
    tx: Sender<BindEvent>,
}

impl ComboEvents {
    pub fn send(&self, action: ActionId, phase: Phase) {
        let _ = self.tx.send(BindEvent {
            action,
            phase,
            t_mono_ns: now_mono_ns(),
        });
    }
    pub fn pressed(&self, action: ActionId) {
        self.send(action, Phase::Down)
    }
    pub fn released(&self, action: ActionId) {
        self.send(action, Phase::Up)
    }
}

pub struct ComboBackend {
    registrar: Box<dyn ComboRegistrar>,
    events: ComboEvents,
    rx: Mutex<Option<Receiver<BindEvent>>>,
}

impl ComboBackend {
    /// Standalone backend with its own event channel.
    pub fn new(registrar: Box<dyn ComboRegistrar>) -> Self {
        let (tx, rx) = channel();
        Self {
            registrar,
            events: ComboEvents { tx },
            rx: Mutex::new(Some(rx)),
        }
    }

    /// Backend that reports into an existing channel (used by `InputManager`).
    pub fn with_sender(registrar: Box<dyn ComboRegistrar>, tx: Sender<BindEvent>) -> Self {
        Self {
            registrar,
            events: ComboEvents { tx },
            rx: Mutex::new(None),
        }
    }

    /// Handle for the shell's shortcut callback.
    pub fn events_handle(&self) -> ComboEvents {
        self.events.clone()
    }

    pub(crate) fn register_one(&mut self, b: &BindSpec) -> std::result::Result<(), String> {
        match &b.trigger {
            Trigger::Combo { mods, key } if crate::keys::is_known_code(&key.0) => {
                self.registrar.register(b.action, *mods, key)
            }
            Trigger::Combo { key, .. } => Err(format!("unknown key {}", key.0)),
            _ => Err("not a plain combo".into()),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.registrar.unregister_all();
    }
}

impl InputBackend for ComboBackend {
    fn set_binds(&mut self, binds: &[BindSpec]) -> Result<BindReport> {
        self.clear();
        let entries = binds
            .iter()
            .map(|b| BindStatusEntry {
                action: b.action,
                status: match self.register_one(b) {
                    Ok(()) => BindStatus::Active,
                    Err(e) => BindStatus::Failed(e),
                },
            })
            .collect();
        Ok(BindReport { entries })
    }

    fn events(&self) -> Receiver<BindEvent> {
        self.rx
            .lock()
            .ok()
            .and_then(|mut g| g.take())
            .unwrap_or_else(|| channel().1)
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Test registrar: records registrations, can refuse chosen actions.
    #[derive(Clone, Default)]
    pub struct FakeRegistrar {
        pub registered: Arc<Mutex<Vec<(ActionId, Mods, String)>>>,
        pub refuse: Arc<Mutex<Vec<ActionId>>>,
    }
    impl ComboRegistrar for FakeRegistrar {
        fn register(&mut self, a: ActionId, m: Mods, k: &KeyCode) -> std::result::Result<(), String> {
            if self.refuse.lock().unwrap().contains(&a) {
                return Err("taken".into());
            }
            self.registered.lock().unwrap().push((a, m, k.0.clone()));
            Ok(())
        }
        fn unregister_all(&mut self) {
            self.registered.lock().unwrap().clear();
        }
    }
}
