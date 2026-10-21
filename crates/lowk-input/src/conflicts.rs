//! Conflict detection: duplicate binds and known reserved OS shortcuts, plus
//! soft warnings for combos that are legal but hijack common app shortcuts.
use lowk_types::{ActionId, BindSpec, KeyCode, Mods, Os, Trigger};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// Same trigger (or same action) already used by an earlier bind; this one is skipped.
    Duplicate { of: ActionId },
    /// Reserved by the OS (English name of the shortcut). The bind is still registered.
    Os(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// Index into the analysed bind slice.
    pub index: usize,
    pub action: ActionId,
    pub kind: ConflictKind,
}

const fn mods(ctrl: bool, alt: bool, shift: bool, meta: bool) -> Mods {
    Mods {
        ctrl,
        alt,
        shift,
        meta,
    }
}
const M: Mods = Mods::META;
const MS: Mods = Mods::META_SHIFT;
const A: Mods = Mods::ALT;
const C: Mods = Mods::CTRL;
const CS: Mods = Mods::CTRL_SHIFT;

/// (mods, W3C key, name)
const MAC: &[(Mods, &str, &str)] = &[
    (MS, "Digit3", "Screenshot (Cmd+Shift+3)"),
    (MS, "Digit4", "Screenshot selection (Cmd+Shift+4)"),
    (MS, "Digit5", "Screenshot and recording (Cmd+Shift+5)"),
    (M, "Space", "Spotlight"),
    (M, "Tab", "App switcher"),
    (M, "KeyQ", "Quit application"),
    (M, "KeyH", "Hide application"),
    (M, "KeyW", "Close window"),
    (M, "Backquote", "Switch windows"),
    (mods(false, true, false, true), "Escape", "Force Quit"),
    (mods(true, false, false, true), "KeyQ", "Lock screen"),
    (mods(true, false, false, false), "Space", "Input source switch"),
];

const WIN: &[(Mods, &str, &str)] = &[
    (M, "KeyX", "Win+X menu"),
    (M, "KeyL", "Lock screen"),
    (M, "KeyD", "Show desktop"),
    (M, "KeyE", "File Explorer"),
    (M, "KeyR", "Run dialog"),
    (M, "Tab", "Task view"),
    (M, "Space", "Input language switch"),
    (MS, "KeyS", "Snipping tool"),
    (A, "Tab", "Window switcher"),
    (A, "F4", "Close window"),
    (CS, "Escape", "Task Manager"),
    (
        mods(true, true, false, false),
        "Delete",
        "Secure attention sequence",
    ),
];

fn table(os: Os) -> &'static [(Mods, &'static str, &'static str)] {
    match os {
        Os::Macos => MAC,
        Os::Windows => WIN,
        Os::Linux => &[],
    }
}

/// Name of the OS shortcut this trigger collides with, if any.
pub fn os_conflict(os: Os, t: &Trigger) -> Option<&'static str> {
    let Trigger::Combo { mods, key } = t else {
        return None;
    };
    table(os)
        .iter()
        .find(|(m, k, _)| m == mods && *k == key.0)
        .map(|e| e.2)
}

/// Legal but hijacks a shortcut many apps use (shown as a warning in the bind row).
pub fn soft_warning(os: Os, t: &Trigger) -> Option<&'static str> {
    let Trigger::Combo { mods, key } = t else {
        return None;
    };
    let primary = if os == Os::Macos { M } else { C };
    let primary_shift = if os == Os::Macos { MS } else { CS };
    match (*mods, key.0.as_str()) {
        (m, "Enter") if m == primary => {
            Some("Taken from every other app while Lowk runs (e.g. send in chat apps)")
        }
        (m, "KeyS") if m == primary_shift => Some("Used as Save As in many apps"),
        (m, k) if m == primary && matches!(k, "KeyC" | "KeyV" | "KeyX" | "KeyZ" | "KeyA") => {
            Some("Clipboard/edit shortcut used by every app")
        }
        _ => None,
    }
}

/// Analyse a bind table (order matters: the first user of a trigger wins).
pub fn analyze(os: Os, binds: &[BindSpec]) -> Vec<Conflict> {
    let mut out = Vec::new();
    for (i, b) in binds.iter().enumerate() {
        let earlier = binds[..i]
            .iter()
            .find(|e| e.same_slot(b) || e.trigger == b.trigger);
        if let Some(e) = earlier {
            out.push(Conflict {
                index: i,
                action: b.action,
                kind: ConflictKind::Duplicate { of: e.action },
            });
            continue;
        }
        if let Some(name) = os_conflict(os, &b.trigger) {
            out.push(Conflict {
                index: i,
                action: b.action,
                kind: ConflictKind::Os(name),
            });
        }
    }
    out
}

/// Convenience used by tests and the settings UI.
pub fn combo(mods: Mods, key: &str) -> Trigger {
    Trigger::Combo {
        mods,
        key: KeyCode::new(key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::default_binds;

    fn spec(a: ActionId, t: Trigger) -> BindSpec {
        BindSpec {
            action: a,
            trigger: t,
            swallow: false,
            template: None,
        }
    }

    #[test]
    fn defaults_have_no_hard_conflicts() {
        for os in [Os::Macos, Os::Windows] {
            assert_eq!(analyze(os, &default_binds(os)), vec![], "{os:?}");
        }
    }

    #[test]
    fn os_shortcuts_detected() {
        let b = [spec(ActionId::Screenshot, combo(MS, "Digit4"))];
        let c = analyze(Os::Macos, &b);
        assert!(matches!(c[0].kind, ConflictKind::Os(_)));
        // same trigger is fine on Windows
        assert!(analyze(Os::Windows, &b).is_empty());
        let w = [spec(ActionId::Ask, combo(M, "KeyX"))];
        assert_eq!(analyze(Os::Windows, &w).len(), 1);
    }

    #[test]
    fn duplicates_detected_first_wins() {
        let b = [
            spec(ActionId::Ask, combo(M, "KeyJ")),
            spec(ActionId::Screenshot, combo(M, "KeyJ")),
            spec(ActionId::Ask, combo(M, "KeyK")),
        ];
        let c = analyze(Os::Macos, &b);
        assert_eq!(c.len(), 2);
        assert_eq!(
            c[0],
            Conflict {
                index: 1,
                action: ActionId::Screenshot,
                kind: ConflictKind::Duplicate { of: ActionId::Ask }
            }
        );
    }

    #[test]
    fn soft_warnings_for_owner_defaults() {
        assert!(soft_warning(Os::Macos, &combo(M, "Enter")).is_some());
        assert!(soft_warning(Os::Windows, &combo(C, "Enter")).is_some());
        assert!(soft_warning(Os::Macos, &combo(MS, "KeyS")).is_some());
        assert!(soft_warning(Os::Macos, &combo(MS, "KeyA")).is_none());
    }

    #[test]
    fn two_template_binds_are_two_slots_but_one_trigger_is_a_conflict() {
        let mut a = spec(ActionId::AskTemplate, combo(Mods::META, "KeyJ"));
        a.template = Some("one".into());
        let mut b = spec(ActionId::AskTemplate, combo(Mods::META, "KeyK"));
        b.template = Some("two".into());
        assert!(analyze(Os::Macos, &[a.clone(), b.clone()]).is_empty());
        // same template twice = same slot
        let mut c = spec(ActionId::AskTemplate, combo(Mods::META, "KeyL"));
        c.template = Some("one".into());
        assert_eq!(analyze(Os::Macos, &[a.clone(), c]).len(), 1);
        // another template on the same trigger = conflict
        let mut d = spec(ActionId::AskTemplate, combo(Mods::META, "KeyJ"));
        d.template = Some("three".into());
        assert_eq!(analyze(Os::Macos, &[a, d]).len(), 1);
    }
}
