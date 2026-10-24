//! «Microphone: auto» detection: does the selected call app itself hold an input (capture) stream?
//!
//! * macOS: Core Audio process objects (`kAudioProcessPropertyIsRunningInput`), grouped by app.
//! * Windows: WASAPI capture sessions (`IAudioSessionManager2`) on the capture endpoints, state Active.
//!
//! The decision itself ([`app_holds_input`]) and the hysteresis ([`Debounce`]) are pure and unit-tested
//! on every OS; the OS probes only build the process rows. Lowk's own process (and anything it
//! started) never counts, so our own microphone stream cannot keep itself open.
use lowk_types::Target;
use std::sync::Arc;

/// One OS process as the detection sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcRow {
    pub pid: u32,
    pub parent: Option<u32>,
    /// Everything the process may be selected by: bundle id, app path, exe path / name.
    pub keys: Vec<String>,
    /// The process has an active input stream right now.
    pub holds_input: bool,
}

/// True when `pid` is `ancestor` or sits below it (walks at most 32 parent links; cycle-safe).
fn within(rows: &[ProcRow], pid: u32, ancestor: u32) -> bool {
    let mut cur = pid;
    for _ in 0..32 {
        if cur == ancestor {
            return true;
        }
        match rows.iter().find(|r| r.pid == cur).and_then(|r| r.parent) {
            Some(p) if p != cur && p > 0 => cur = p,
            _ => return false,
        }
    }
    false
}

fn spec_matches(row: &ProcRow, spec: &str) -> bool {
    if let Ok(pid) = spec.parse::<u32>() {
        return row.pid == pid;
    }
    let want = spec.to_lowercase();
    row.keys.iter().any(|k| {
        let k = k.to_lowercase();
        !k.is_empty()
            && (k == want || k.starts_with(&format!("{want}.")) || k.trim_end_matches(".exe") == want)
    })
}

/// Whether the app selected by `spec` (bundle id, exe path / name or pid, as in `Target::App`) holds an
/// input stream: any process of the app or any process below it (helpers, the process tree) with an
/// active capture stream. `own_pid` and everything below it is ignored.
pub fn app_holds_input(rows: &[ProcRow], spec: &str, own_pid: u32) -> bool {
    let candidates: Vec<&ProcRow> = rows.iter().filter(|r| !within(rows, r.pid, own_pid)).collect();
    let seeds: Vec<u32> = candidates
        .iter()
        .filter(|r| spec_matches(r, spec))
        .map(|r| r.pid)
        .collect();
    candidates
        .iter()
        .any(|r| r.holds_input && seeds.iter().any(|s| within(rows, r.pid, *s)))
}

/// Hysteresis for the raw 1 Hz readings: the state turns on after the input has been held for `on_after_ms`
/// and off after it has been released for `off_after_ms`.
#[derive(Clone, Debug)]
pub struct Debounce {
    on_after_ms: u64,
    off_after_ms: u64,
    current: bool,
    /// When the raw reading started to differ from `current`.
    since: Option<u64>,
}

impl Debounce {
    pub fn new(initial: bool, on_after_ms: u64, off_after_ms: u64) -> Self {
        Self {
            on_after_ms,
            off_after_ms,
            current: initial,
            since: None,
        }
    }
    pub fn current(&self) -> bool {
        self.current
    }
    /// Feeds one reading; returns the new state when it switched.
    pub fn update(&mut self, now_ms: u64, raw: bool) -> Option<bool> {
        if raw == self.current {
            self.since = None;
            return None;
        }
        let since = *self.since.get_or_insert(now_ms);
        let need = if raw { self.on_after_ms } else { self.off_after_ms };
        if now_ms.saturating_sub(since) >= need {
            self.current = raw;
            self.since = None;
            return Some(raw);
        }
        None
    }
}

/// Asks the OS whether an app holds the microphone.
pub trait MicUseProbe: Send + Sync {
    /// False where detection does not exist (other OSes, tests without a fake): auto then behaves like "on".
    fn supported(&self) -> bool;
    /// `Some(held)` for an app target; `None` when this reading failed (the caller keeps the last state).
    fn app_holds_input(&self, target: &Target) -> Option<bool>;
}

/// No detection.
pub struct NoMicUseProbe;
impl MicUseProbe for NoMicUseProbe {
    fn supported(&self) -> bool {
        false
    }
    fn app_holds_input(&self, _t: &Target) -> Option<bool> {
        None
    }
}

/// The OS probe of this platform (unsupported elsewhere).
pub fn default_probe() -> Arc<dyn MicUseProbe> {
    #[cfg(target_os = "macos")]
    return Arc::new(MacProbe);
    #[cfg(windows)]
    return Arc::new(WinProbe);
    #[allow(unreachable_code)]
    Arc::new(NoMicUseProbe)
}

/// A scripted probe for tests of dependants: the answer is set from outside.
#[derive(Clone, Default)]
pub struct FakeMicUseProbe(Arc<std::sync::Mutex<Option<bool>>>);
impl FakeMicUseProbe {
    pub fn new(held: bool) -> Self {
        Self(Arc::new(std::sync::Mutex::new(Some(held))))
    }
    pub fn set(&self, held: Option<bool>) {
        *self.0.lock().unwrap() = held;
    }
}
impl MicUseProbe for FakeMicUseProbe {
    fn supported(&self) -> bool {
        true
    }
    fn app_holds_input(&self, _t: &Target) -> Option<bool> {
        *self.0.lock().unwrap()
    }
}

#[cfg(target_os = "macos")]
struct MacProbe;
#[cfg(target_os = "macos")]
impl MicUseProbe for MacProbe {
    fn supported(&self) -> bool {
        true
    }
    fn app_holds_input(&self, target: &Target) -> Option<bool> {
        let Target::App(spec) = target else { return None };
        let groups = crate::mac::procs::groups().ok()?;
        let mut rows = vec![];
        for g in &groups {
            for m in &g.members {
                let mut keys = vec![g.bundle_id.clone(), g.name.clone(), g.key.clone(), m.path.clone()];
                keys.push(m.bundle_id.clone());
                rows.push(ProcRow {
                    pid: m.pid.max(0) as u32,
                    parent: Some(crate::mac::procs::parent_pid(m.pid))
                        .filter(|p| *p > 1)
                        .map(|p| p as u32),
                    keys,
                    holds_input: m.running_input,
                });
            }
        }
        Some(app_holds_input(&rows, spec, std::process::id()))
    }
}

#[cfg(windows)]
struct WinProbe;
#[cfg(windows)]
impl MicUseProbe for WinProbe {
    fn supported(&self) -> bool {
        true
    }
    fn app_holds_input(&self, target: &Target) -> Option<bool> {
        let Target::App(spec) = target else { return None };
        crate::win::capture_session_rows().map(|rows| app_holds_input(&rows, spec, std::process::id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: u32, parent: Option<u32>, keys: &[&str], holds: bool) -> ProcRow {
        ProcRow {
            pid,
            parent,
            keys: keys.iter().map(|k| k.to_string()).collect(),
            holds_input: holds,
        }
    }

    const OWN: u32 = 900;

    #[test]
    fn main_process_of_the_selected_app_holding_input_counts() {
        let rows = [
            row(10, Some(1), &["us.zoom.xos"], true),
            row(20, Some(1), &["com.other"], false),
        ];
        assert!(app_holds_input(&rows, "us.zoom.xos", OWN));
        assert!(!app_holds_input(&rows, "com.other", OWN));
    }

    #[test]
    fn helper_processes_of_the_app_count() {
        // Chromium style: the capture stream lives in a helper that has its own bundle id and is a child
        let rows = [
            row(10, Some(1), &["com.google.Chrome"], false),
            row(11, Some(10), &["com.google.Chrome.helper"], true),
        ];
        assert!(app_holds_input(&rows, "com.google.Chrome", OWN));
        // the helper is also matched by the bundle-id prefix without any parent link
        let rows = [row(11, None, &["com.google.Chrome.helper.Renderer"], true)];
        assert!(app_holds_input(&rows, "com.google.Chrome", OWN));
    }

    #[test]
    fn process_tree_of_a_windows_exe_counts_and_other_apps_do_not() {
        let rows = [
            row(100, Some(4), &["c:\\apps\\tolk.exe", "tolk.exe"], false),
            row(101, Some(100), &["c:\\apps\\tolk.exe", "tolk.exe"], false),
            row(102, Some(101), &["audiohost.exe"], true), // grandchild with another exe name
            row(200, Some(4), &["chrome.exe"], true),      // somebody else holds the mic
        ];
        assert!(app_holds_input(&rows, "c:\\apps\\tolk.exe", OWN));
        assert!(app_holds_input(&rows, "Tolk", OWN));
        assert!(app_holds_input(&rows, "100", OWN), "a pid spec selects its tree");
        let other_only = [rows[0].clone(), rows[3].clone()];
        assert!(!app_holds_input(&other_only, "tolk.exe", OWN));
    }

    #[test]
    fn lowks_own_process_and_children_are_excluded() {
        let rows = [
            row(OWN, Some(1), &["app.lowk"], true),
            row(901, Some(OWN), &["app.lowk.webview"], true),
            row(10, Some(1), &["us.zoom.xos"], false),
        ];
        assert!(!app_holds_input(&rows, "us.zoom.xos", OWN));
        assert!(
            !app_holds_input(&rows, "app.lowk", OWN),
            "selecting Lowk itself never counts"
        );
        // a call app started by Lowk (child of our process) is not seen either
        let rows = [
            row(OWN, Some(1), &["app.lowk"], false),
            row(5, Some(OWN), &["x"], true),
        ];
        assert!(!app_holds_input(&rows, "x", OWN));
    }

    #[test]
    fn unknown_app_and_pid_cycles_are_safe() {
        assert!(!app_holds_input(&[], "x", OWN));
        let rows = [row(1, Some(2), &["a"], true), row(2, Some(1), &["b"], true)];
        assert!(!app_holds_input(&rows, "zzz", OWN));
        assert!(app_holds_input(&rows, "a", OWN));
    }

    #[test]
    fn hysteresis_on_after_1s_off_after_3s() {
        let mut d = Debounce::new(false, 1000, 3000);
        assert_eq!(d.update(0, true), None);
        assert_eq!(d.update(900, true), None);
        assert_eq!(d.update(1000, true), Some(true), "held for 1 s: on");
        assert_eq!(d.update(2000, true), None);
        // a short release does not switch off ...
        assert_eq!(d.update(3000, false), None);
        assert_eq!(d.update(5000, false), None);
        assert_eq!(d.update(5500, true), None, "held again: the off timer resets");
        assert_eq!(d.update(6000, false), None);
        assert_eq!(d.update(8999, false), None);
        assert_eq!(d.update(9000, false), Some(false), "released for 3 s: off");
        // a blip shorter than 1 s never switches on
        assert_eq!(d.update(10_000, true), None);
        assert_eq!(d.update(10_500, false), None);
        assert_eq!(d.update(11_500, true), None);
        assert_eq!(d.update(12_499, true), None);
        assert!(!d.current());
    }
}
