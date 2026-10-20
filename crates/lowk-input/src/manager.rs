//! `InputManager`: the one object the app holds. Routes each bind to the best
//! backend, merges events into one channel, produces the `BindReport`.
//!
//! Routing: plain `Combo` without swallow -> combo adapter (no permission) when
//! the shell provided one; everything else (and combos the adapter refused) ->
//! low-level hook. A hook failure never panics: affected binds are reported
//! `Degraded(permission)` / `Failed` and everything else keeps working.
use crate::combo::{ComboBackend, ComboEvents, ComboRegistrar};
use crate::conflicts::{analyze, ConflictKind};
use crate::cursor::SystemCursor;
use crate::lowlevel::LowLevelBackend;
use crate::platform::PermSnapshot;
use lowk_types::{
    ActionId, BindEvent, BindReport, BindSpec, BindStatus, BindStatusEntry, CursorSource, InputBackend, Os,
    PermKind, Result, Trigger,
};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

/// What works right now; shown in Settings and onboarding.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputStatus {
    pub combo_adapter: bool,
    /// Low-level hook thread is running.
    pub hook_running: bool,
    /// Running hook can swallow events.
    pub can_swallow: bool,
    pub permissions: PermSnapshot,
    /// Binds that still work although the hook is unavailable (combos via the adapter).
    pub working_without_hook: Vec<ActionId>,
    /// Binds that need the hook and are therefore not working.
    pub needs_hook: Vec<ActionId>,
}

pub struct InputManager {
    os: Os,
    combo: Option<ComboBackend>,
    low: LowLevelBackend,
    rx: Mutex<Option<Receiver<BindEvent>>>,
    status: InputStatus,
}

pub type InputHandle = InputManager;

impl InputManager {
    /// `registrar`: the shell's global-shortcut adapter, `None` when unavailable.
    pub fn new(registrar: Option<Box<dyn ComboRegistrar>>) -> Self {
        Self::with_os(Os::current(), registrar)
    }

    pub fn with_os(os: Os, registrar: Option<Box<dyn ComboRegistrar>>) -> Self {
        let (tx, rx) = channel();
        Self {
            os,
            combo: registrar.map(|r| ComboBackend::with_sender(r, tx.clone())),
            low: LowLevelBackend::with_sender(tx),
            rx: Mutex::new(Some(rx)),
            status: InputStatus::default(),
        }
    }

    /// Handle the shell's shortcut callback must call (`pressed`/`released`).
    pub fn combo_events(&self) -> Option<ComboEvents> {
        self.combo.as_ref().map(|c| c.events_handle())
    }

    pub fn status(&self) -> &InputStatus {
        &self.status
    }

    pub fn cursor(&self) -> Arc<dyn CursorSource> {
        Arc::new(SystemCursor)
    }

    /// macOS: show the Input Monitoring prompt (no-op elsewhere). Call from onboarding.
    pub fn request_input_monitoring(&self) -> bool {
        crate::platform::request_input_monitoring()
    }

    /// Start capturing the next combo / modifier-only / mouse button for the settings UI.
    pub fn record_start(&mut self) -> Result<Receiver<Trigger>> {
        self.low.start_record()
    }

    pub fn record_stop(&mut self) {
        self.low.stop_record();
    }
}

impl InputBackend for InputManager {
    fn set_binds(&mut self, binds: &[BindSpec]) -> Result<BindReport> {
        let mut status: Vec<Option<BindStatus>> = vec![None; binds.len()];
        let mut os_note: Vec<Option<&'static str>> = vec![None; binds.len()];

        // 1. conflicts: duplicates are skipped, OS-reserved ones still registered.
        let mut skip = vec![false; binds.len()];
        for c in analyze(self.os, binds) {
            match c.kind {
                ConflictKind::Duplicate { of } => {
                    skip[c.index] = true;
                    status[c.index] = Some(BindStatus::Conflict(format!("duplicate of {of:?}")));
                }
                ConflictKind::Os(name) => os_note[c.index] = Some(name),
            }
        }

        // 2. route.
        let mut via_low: Vec<usize> = Vec::new();
        if let Some(c) = self.combo.as_mut() {
            c.clear();
        }
        for (i, b) in binds.iter().enumerate() {
            if skip[i] {
                continue;
            }
            let plain = matches!(b.trigger, Trigger::Combo { .. }) && !b.swallow;
            match (plain, self.combo.as_mut()) {
                (true, Some(c)) => match c.register_one(b) {
                    Ok(()) => status[i] = Some(BindStatus::Active),
                    Err(_) => via_low.push(i), // taken/refused: try the hook
                },
                _ => via_low.push(i),
            }
        }
        let low_binds: Vec<BindSpec> = via_low.iter().map(|&i| binds[i].clone()).collect();
        let low_status = self.low.apply(&low_binds);
        for (&i, s) in via_low.iter().zip(low_status) {
            status[i] = Some(s);
        }

        // 3. OS-conflict note overrides Active (still registered, UI shows warning).
        for (i, note) in os_note.iter().enumerate() {
            if let (Some(n), Some(BindStatus::Active)) = (note, &status[i]) {
                status[i] = Some(BindStatus::Conflict((*n).to_owned()));
            }
        }

        // 4. status summary.
        let info = self.low.hook_info();
        let mut st = InputStatus {
            combo_adapter: self.combo.is_some(),
            hook_running: info.is_some(),
            can_swallow: info.is_some_and(|i| i.swallow_capable),
            permissions: self.low.permissions(),
            ..Default::default()
        };
        for (i, b) in binds.iter().enumerate() {
            let via_hook = via_low.contains(&i);
            match (&status[i], via_hook, info.is_some()) {
                (Some(BindStatus::Degraded(PermKind::InputMonitoring)), _, _)
                | (Some(BindStatus::Failed(_)), true, _) => st.needs_hook.push(b.action),
                (Some(BindStatus::Active | BindStatus::Conflict(_)), false, _) => {
                    st.working_without_hook.push(b.action)
                }
                _ => {}
            }
        }
        self.status = st;

        Ok(BindReport {
            entries: binds
                .iter()
                .zip(status)
                .map(|(b, s)| BindStatusEntry {
                    action: b.action,
                    status: s.unwrap_or(BindStatus::Failed("not routed".into())),
                })
                .collect(),
        })
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
mod tests {
    use super::*;
    use crate::combo::fake::FakeRegistrar;
    use lowk_types::{default_binds, Phase};

    #[test]
    fn plain_combos_go_to_adapter_others_to_hook() {
        let reg = FakeRegistrar::default();
        let registered = reg.registered.clone();
        let mut m = InputManager::with_os(Os::Macos, Some(Box::new(reg)));
        let rep = m.set_binds(&default_binds(Os::Macos)).unwrap();
        assert_eq!(registered.lock().unwrap().len(), 6);
        let mv = rep
            .entries
            .iter()
            .find(|e| e.action == ActionId::MoveHold)
            .unwrap();
        // MoveHold goes to the hook: Active, Degraded(InputMonitoring) or Failed, never a combo.
        assert!(!matches!(mv.status, BindStatus::Conflict(_)));
        assert!(m.status().combo_adapter);
        for e in rep.entries.iter().filter(|e| e.action != ActionId::MoveHold) {
            assert_eq!(e.status, BindStatus::Active, "{:?}", e.action);
        }
    }

    #[test]
    fn refused_combo_falls_back_and_duplicates_conflict() {
        let reg = FakeRegistrar::default();
        reg.refuse.lock().unwrap().push(ActionId::Ask);
        let mut m = InputManager::with_os(Os::Windows, Some(Box::new(reg)));
        let mut binds = default_binds(Os::Windows);
        binds.push(binds[1].clone()); // same action twice
        let rep = m.set_binds(&binds).unwrap();
        assert!(matches!(
            rep.entries.last().unwrap().status,
            BindStatus::Conflict(_)
        ));
        let ask = &rep.entries[0];
        assert!(!matches!(ask.status, BindStatus::Active) || m.status().hook_running);
    }

    #[test]
    fn combo_events_reach_merged_channel() {
        let mut m = InputManager::with_os(Os::Macos, Some(Box::new(FakeRegistrar::default())));
        let rx = m.events();
        m.set_binds(&[]).unwrap();
        let h = m.combo_events().unwrap();
        h.pressed(ActionId::Ask);
        h.released(ActionId::Ask);
        assert_eq!(rx.try_recv().unwrap().phase, Phase::Down);
        assert_eq!(rx.try_recv().unwrap().phase, Phase::Up);
    }

    #[test]
    fn without_adapter_everything_goes_to_hook() {
        let mut m = InputManager::with_os(Os::current(), None);
        let rep = m.set_binds(&default_binds(Os::current())).unwrap();
        assert_eq!(rep.entries.len(), 7);
        assert!(!m.status().combo_adapter);
    }
}
