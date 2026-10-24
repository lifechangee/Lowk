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
    fn register(
        &mut self,
        action: ActionId,
        template: Option<&str>,
        mods: Mods,
        key: &KeyCode,
    ) -> std::result::Result<(), String>;
    /// Drop every registration made through this registrar.
    fn unregister_all(&mut self);
}

/// Clonable handle the shell calls from its shortcut handler.
#[derive(Clone)]
pub struct ComboEvents {
    tx: Sender<BindEvent>,
}

impl ComboEvents {
    pub fn send(&self, action: ActionId, template: Option<String>, phase: Phase) {
        let _ = self.tx.send(BindEvent {
            action,
            phase,
            t_mono_ns: now_mono_ns(),
            template,
        });
    }
    pub fn pressed(&self, action: ActionId, template: Option<String>) {
        self.send(action, template, Phase::Down)
    }
    pub fn released(&self, action: ActionId, template: Option<String>) {
        self.send(action, template, Phase::Up)
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
                self.registrar
                    .register(b.action, b.template.as_deref(), *mods, key)
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
                template: b.template.clone(),
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
        fn register(
            &mut self,
            a: ActionId,
            _t: Option<&str>,
            m: Mods,
            k: &KeyCode,
        ) -> std::result::Result<(), String> {
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

/// Owner bug: ⌘⇧S opened the area selection (`ScreenshotArea`). The owner's real `settings.toml` binds
/// `screenshot` to ⌘⇧S and `screenshot_area` to ⌘⇧A; every layer from the file to the registered shortcut and
/// the low-level matcher must keep that mapping.
#[cfg(test)]
mod owner_binds {
    use super::fake::FakeRegistrar;
    use super::*;
    use crate::matcher::{Matcher, Raw};
    use lowk_types::Settings;

    fn owner_binds() -> Vec<BindSpec> {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../lowk-store/tests/fixtures/owner-settings.toml"
        ))
        .unwrap();
        let t: toml::Value = toml::from_str(&text).unwrap();
        let s: Settings = serde_json::from_value(serde_json::to_value(t).unwrap()).unwrap();
        s.binds
    }

    #[test]
    fn owner_toml_registers_cmd_shift_s_for_screenshot_and_cmd_shift_a_for_area() {
        let binds = owner_binds();
        let reg = FakeRegistrar::default();
        let registered = reg.registered.clone();
        let mut c = ComboBackend::new(Box::new(reg));
        let rep = c.set_binds(&binds).unwrap();
        assert!(rep
            .entries
            .iter()
            .filter(|e| matches!(e.action, ActionId::Screenshot | ActionId::ScreenshotArea))
            .all(|e| e.status == BindStatus::Active));
        let reg = registered.lock().unwrap();
        let key_of = |a: ActionId| reg.iter().find(|r| r.0 == a).map(|r| (r.1, r.2.clone()));
        assert_eq!(
            key_of(ActionId::Screenshot),
            Some((Mods::META_SHIFT, "KeyS".into()))
        );
        assert_eq!(
            key_of(ActionId::ScreenshotArea),
            Some((Mods::META_SHIFT, "KeyA".into()))
        );
    }

    #[test]
    fn owner_toml_binds_dispatch_screenshot_for_cmd_shift_s_and_area_for_cmd_shift_a() {
        let mut m = Matcher::default();
        m.set_binds(owner_binds());
        let key = |code: &'static str, down| Raw::Key {
            code,
            down,
            repeat: false,
        };
        m.handle(Raw::Mods(Mods::META_SHIFT));
        let fired =
            |o: crate::matcher::Outcome| o.events.iter().map(|e| (e.action, e.phase)).collect::<Vec<_>>();
        assert_eq!(
            fired(m.handle(key("KeyS", true))),
            vec![(ActionId::Screenshot, Phase::Down)]
        );
        assert_eq!(
            fired(m.handle(key("KeyS", false))),
            vec![(ActionId::Screenshot, Phase::Up)]
        );
        assert_eq!(
            fired(m.handle(key("KeyA", true))),
            vec![(ActionId::ScreenshotArea, Phase::Down)]
        );
        assert_eq!(
            fired(m.handle(key("KeyA", false))),
            vec![(ActionId::ScreenshotArea, Phase::Up)]
        );
        // the physical key codes behind them (macOS kVK / Windows VK) name the same keys
        assert_eq!(crate::keys::from_mac_vk(1), "KeyS");
        assert_eq!(crate::keys::from_mac_vk(0), "KeyA");
        assert_eq!(crate::keys::from_win_vk(0x53), "KeyS");
        assert_eq!(crate::keys::from_win_vk(0x41), "KeyA");
    }
}
