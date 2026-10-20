//! `ComboRegistrar` over `tauri-plugin-global-shortcut` (no permission needed).
//! Plain combos only; hold / modifier-only / mouse binds are `lowk-input`'s own hooks.
use lowk_input::{ComboEvents, ComboRegistrar};
use lowk_types::{ActionId, KeyCode, Mods};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::OnceLock;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};

/// Shared between the registrar (which maps shortcut ids to actions) and the
/// plugin's handler (which reports presses). Filled once at startup.
#[derive(Default)]
pub struct ComboHub {
    ids: Mutex<HashMap<u32, ActionId>>,
    events: Mutex<Option<ComboEvents>>,
}

pub fn hub() -> &'static ComboHub {
    static HUB: OnceLock<ComboHub> = OnceLock::new();
    HUB.get_or_init(ComboHub::default)
}

impl ComboHub {
    pub fn set_events(&self, e: ComboEvents) {
        *self.events.lock() = Some(e);
    }
}

/// The plugin-wide handler (installed by `Builder::with_handler`).
pub fn on_shortcut(_app: &AppHandle, shortcut: &Shortcut, ev: ShortcutEvent) {
    let h = hub();
    let Some(action) = h.ids.lock().get(&shortcut.id()).copied() else {
        return;
    };
    if let Some(e) = h.events.lock().as_ref() {
        match ev.state {
            ShortcutState::Pressed => e.pressed(action),
            ShortcutState::Released => e.released(action),
        }
    }
}

pub struct TauriCombo {
    app: AppHandle,
}

impl TauriCombo {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

fn modifiers(m: Mods) -> Option<Modifiers> {
    let mut out = Modifiers::empty();
    if m.ctrl {
        out |= Modifiers::CONTROL;
    }
    if m.alt {
        out |= Modifiers::ALT;
    }
    if m.shift {
        out |= Modifiers::SHIFT;
    }
    if m.meta {
        out |= Modifiers::SUPER;
    }
    (!out.is_empty()).then_some(out)
}

impl ComboRegistrar for TauriCombo {
    fn register(&mut self, action: ActionId, mods: Mods, key: &KeyCode) -> Result<(), String> {
        let code: Code = key.0.parse().map_err(|_| format!("unknown key {}", key.0))?;
        let sc = Shortcut::new(modifiers(mods), code);
        hub().ids.lock().insert(sc.id(), action);
        self.app.global_shortcut().register(sc).map_err(|e| e.to_string())
    }

    fn unregister_all(&mut self) {
        hub().ids.lock().clear();
        let _ = self.app.global_shortcut().unregister_all();
    }
}
