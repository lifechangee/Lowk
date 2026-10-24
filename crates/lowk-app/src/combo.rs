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
    ids: Mutex<HashMap<u32, (ActionId, Option<String>)>>,
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

    fn bind(&self, sc: &Shortcut, action: ActionId, template: Option<&str>) {
        self.ids
            .lock()
            .insert(sc.id(), (action, template.map(str::to_owned)));
    }

    fn lookup(&self, id: u32) -> Option<(ActionId, Option<String>)> {
        self.ids.lock().get(&id).cloned()
    }
}

/// The plugin-wide handler (installed by `Builder::with_handler`).
pub fn on_shortcut(_app: &AppHandle, shortcut: &Shortcut, ev: ShortcutEvent) {
    let h = hub();
    let Some((action, template)) = h.lookup(shortcut.id()) else {
        return;
    };
    if ev.state == ShortcutState::Pressed {
        // Proves in lowk.log which physical combo the OS reported for which action (bind-mapping diagnostics).
        tracing::info!(shortcut = %shortcut.into_string(), ?action, "global shortcut");
    }
    if let Some(e) = h.events.lock().as_ref() {
        match ev.state {
            ShortcutState::Pressed => e.pressed(action, template),
            ShortcutState::Released => e.released(action, template),
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

fn shortcut_of(mods: Mods, key: &KeyCode) -> Result<Shortcut, String> {
    let code: Code = key.0.parse().map_err(|_| format!("unknown key {}", key.0))?;
    Ok(Shortcut::new(modifiers(mods), code))
}

impl ComboRegistrar for TauriCombo {
    fn register(
        &mut self,
        action: ActionId,
        template: Option<&str>,
        mods: Mods,
        key: &KeyCode,
    ) -> Result<(), String> {
        let sc = shortcut_of(mods, key)?;
        hub().bind(&sc, action, template);
        self.app.global_shortcut().register(sc).map_err(|e| e.to_string())
    }

    fn unregister_all(&mut self) {
        hub().ids.lock().clear();
        let _ = self.app.global_shortcut().unregister_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Owner bug: ⌘⇧S arrived as `ScreenshotArea`. Whatever the OS reports for a registered shortcut id must
    /// resolve to the action that owns that exact combo (owner's binds, a private hub).
    #[test]
    fn shortcut_ids_route_cmd_shift_s_to_screenshot_and_cmd_shift_a_to_area() {
        let hub = ComboHub::default();
        let ms = Mods::META_SHIFT;
        let all = [
            (ActionId::AskBrief, Mods::META, "Enter"),
            (ActionId::Screenshot, ms, "KeyS"),
            (ActionId::ScreenshotArea, ms, "KeyA"),
            (ActionId::ToggleInteractive, ms, "KeyI"),
            (ActionId::ToggleOverlay, Mods::META, "Backslash"),
            (ActionId::PanicHide, ms, "KeyH"),
            (ActionId::ToggleRecord, ms, "KeyR"),
        ];
        for (a, m, k) in all {
            hub.bind(&shortcut_of(m, &KeyCode::new(k)).unwrap(), a, None);
        }
        for (a, m, k) in all {
            let sc = shortcut_of(m, &KeyCode::new(k)).unwrap();
            assert_eq!(hub.lookup(sc.id()).map(|x| x.0), Some(a), "{k}");
        }
        let ids: std::collections::HashSet<u32> = all
            .iter()
            .map(|(_, m, k)| shortcut_of(*m, &KeyCode::new(k)).unwrap().id())
            .collect();
        assert_eq!(ids.len(), all.len(), "every combo has its own shortcut id");
    }
}
