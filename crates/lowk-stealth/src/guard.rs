//! `StealthGuard`: apply -> verify (read-back) -> show, fail closed.
//!
//! Callable from any thread; every native call is marshalled to the main
//! thread through the injected [`MainThread`]. The watchdog thread only
//! *decides* (it posts a verify job), the main thread *applies*.

use crate::backend::{unavailable, Readback, WindowBackend};
use crate::policy::WindowPolicy;
use lowk_types::{AppError, AppEvent, ErrorCode, EventSink, MainThread, Result, StealthStatus};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

/// Run `f` on the main thread and wait for the result (inline if already there).
pub fn run_main_sync<R: Send + 'static>(
    main: &Arc<dyn MainThread>,
    f: impl FnOnce() -> R + Send + 'static,
) -> R {
    if main.is_main() {
        return f();
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    main.run(Box::new(move || {
        let _ = tx.send(f());
    }));
    rx.recv().expect("main thread dropped the job")
}

struct Win {
    policy: Mutex<WindowPolicy>,
    backend: Arc<dyn WindowBackend>,
    /// We believe the window is shown (drives the watchdog).
    visible: AtomicBool,
    /// Visible at the moment panic hide engaged.
    was_visible: AtomicBool,
    verify_inflight: AtomicBool,
}

#[derive(Default)]
struct Notifier {
    sink: Mutex<Option<Arc<dyn EventSink>>>,
    last: Mutex<Option<StealthStatus>>,
}

struct Inner {
    main: Arc<dyn MainThread>,
    wins: Mutex<HashMap<String, Arc<Win>>>,
    status: Mutex<HashMap<String, StealthStatus>>,
    panic: AtomicBool,
    notifier: Notifier,
}

#[derive(Clone)]
pub struct StealthGuard {
    inner: Arc<Inner>,
}

impl StealthGuard {
    pub fn new(main: Arc<dyn MainThread>) -> Self {
        Self {
            inner: Arc::new(Inner {
                main,
                wins: Mutex::new(HashMap::new()),
                status: Mutex::new(HashMap::new()),
                panic: AtomicBool::new(false),
                notifier: Notifier::default(),
            }),
        }
    }

    pub fn set_sink(&self, sink: Arc<dyn EventSink>) {
        *self.inner.notifier.sink.lock() = Some(sink);
    }

    /// Register a native window under `label` with `policy`: prepare + apply +
    /// verify. Any helper window MUST go through here before it is ever shown.
    pub fn protect_backend(
        &self,
        label: &str,
        policy: WindowPolicy,
        backend: Arc<dyn WindowBackend>,
    ) -> Result<()> {
        let win = Arc::new(Win {
            policy: Mutex::new(policy),
            backend,
            visible: AtomicBool::new(false),
            was_visible: AtomicBool::new(false),
            verify_inflight: AtomicBool::new(false),
        });
        self.inner.wins.lock().insert(label.to_string(), win.clone());
        let this = self.clone();
        let label2 = label.to_string();
        let r = run_main_sync(&self.inner.main, move || {
            win.backend.prepare(&policy)?;
            let r = this.apply_core(&win);
            this.record(&label2, &r);
            r.map(|_| ())
        });
        if let Err(e) = &r {
            tracing::error!(label, %e, "protect failed");
        }
        r
    }

    /// Change the policy of one label (e.g. the "hide main from capture"
    /// setting) and apply it. Other labels are untouched.
    pub fn set_policy(&self, label: &str, policy: WindowPolicy) -> Result<()> {
        let win = self.win(label)?;
        *win.policy.lock() = policy;
        self.apply(label)
    }

    pub fn apply(&self, label: &str) -> Result<()> {
        let win = self.win(label)?;
        let this = self.clone();
        let label2 = label.to_string();
        run_main_sync(&self.inner.main, move || {
            let r = this.apply_core(&win);
            this.record(&label2, &r);
            r.map(|_| ())
        })
    }

    /// Read-back only. `Degraded` means the flag drifted (a re-apply fixes it).
    pub fn verify(&self, label: &str) -> StealthStatus {
        let Ok(win) = self.win(label) else {
            return StealthStatus::Unavailable("unknown window".into());
        };
        run_main_sync(&self.inner.main, move || verify_core(&win))
    }

    /// apply -> verify -> only then show. While panic hide is engaged, stealth
    /// windows stay hidden and this returns `Ok` without showing.
    pub fn show(&self, label: &str) -> Result<()> {
        let win = self.win(label)?;
        let policy = *win.policy.lock();
        if policy.is_stealth() && self.inner.panic.load(Ordering::SeqCst) {
            win.was_visible.store(true, Ordering::SeqCst);
            tracing::info!(label, "show blocked by panic hide");
            return Ok(());
        }
        let this = self.clone();
        let label2 = label.to_string();
        run_main_sync(&self.inner.main, move || this.show_core(&label2, &win))
    }

    pub fn hide(&self, label: &str) {
        if let Ok(win) = self.win(label) {
            win.visible.store(false, Ordering::SeqCst);
            win.was_visible.store(false, Ordering::SeqCst);
            run_main_sync(&self.inner.main, move || win.backend.hide());
        }
    }

    pub fn is_visible(&self, label: &str) -> bool {
        self.win(label)
            .map(|w| w.visible.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    pub fn panic_active(&self) -> bool {
        self.inner.panic.load(Ordering::SeqCst)
    }

    /// Toggle panic hide. Engage: every stealth window is hidden natively on
    /// the main thread (no JS round-trip) and re-show is blocked. Disengage:
    /// windows that were visible come back through apply -> verify -> show.
    pub fn panic_hide(&self) {
        let engage = !self.inner.panic.fetch_xor(true, Ordering::SeqCst);
        let this = self.clone();
        let job = move || {
            let wins: Vec<(String, Arc<Win>)> = this
                .inner
                .wins
                .lock()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (label, w) in wins {
                let policy = *w.policy.lock();
                if !policy.is_stealth() {
                    continue;
                }
                if engage {
                    w.was_visible
                        .store(w.visible.swap(false, Ordering::SeqCst), Ordering::SeqCst);
                    w.backend.hide();
                } else if w.was_visible.swap(false, Ordering::SeqCst) {
                    if let Err(e) = this.show_core(&label, &w) {
                        tracing::error!(%label, %e, "re-show after panic failed closed");
                    }
                }
            }
        };
        if self.inner.main.is_main() {
            job();
        } else {
            self.inner.main.run(Box::new(job));
        }
    }

    /// Re-apply every window and re-assert level/topmost: after activation
    /// policy flips, display changes, wake, Space changes, foreground changes.
    pub fn on_policy_change(&self) {
        let this = self.clone();
        let job = move || {
            let wins: Vec<(String, Arc<Win>)> = this
                .inner
                .wins
                .lock()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (label, w) in wins {
                this.reassert_core(&label, &w);
            }
        };
        if self.inner.main.is_main() {
            job()
        } else {
            self.inner.main.run(Box::new(job));
        }
    }

    /// One watchdog pass: for each visible window post a verify job; drift is
    /// re-applied, an unrecoverable failure hides the window (fail closed).
    pub fn watchdog_tick(&self) {
        let wins: Vec<(String, Arc<Win>)> = self
            .inner
            .wins
            .lock()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (label, w) in wins {
            if !w.visible.load(Ordering::SeqCst) || w.verify_inflight.swap(true, Ordering::SeqCst) {
                continue;
            }
            let this = self.clone();
            self.inner.main.run(Box::new(move || {
                this.reassert_core(&label, &w);
                w.verify_inflight.store(false, Ordering::SeqCst);
            }));
        }
    }

    /// Spawn the watchdog thread (stops when the guard is dropped).
    pub fn spawn_watchdog(&self, interval: Duration) -> std::thread::JoinHandle<()> {
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        std::thread::Builder::new()
            .name("lowk-stealth-watchdog".into())
            .spawn(move || loop {
                std::thread::sleep(interval);
                let Some(inner) = weak.upgrade() else { return };
                StealthGuard { inner }.watchdog_tick();
            })
            .expect("spawn watchdog")
    }

    /// Aggregate status over all capture-excluded windows.
    pub fn status(&self) -> StealthStatus {
        let st = self.inner.status.lock();
        let wins = self.inner.wins.lock();
        let mut worst = StealthStatus::Verified;
        for (label, w) in wins.iter() {
            if !w.policy.lock().capture_excluded {
                continue;
            }
            match st.get(label) {
                Some(s @ StealthStatus::Unavailable(_)) => return s.clone(),
                Some(s @ StealthStatus::Degraded(_)) => worst = s.clone(),
                Some(StealthStatus::Verified) => {}
                None => worst = StealthStatus::Degraded(format!("{label}: not verified yet")),
            }
        }
        worst
    }

    // ---- internals (main thread) -------------------------------------------------

    fn win(&self, label: &str) -> Result<Arc<Win>> {
        self.inner
            .wins
            .lock()
            .get(label)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail(format!("window {label}")))
    }

    /// apply, and if the read-back disagrees apply once more, then give up.
    fn apply_core(&self, win: &Win) -> Result<Readback> {
        let policy = *win.policy.lock();
        let mut rb = win.backend.apply(&policy)?;
        if !rb.matches(&policy) {
            tracing::warn!(detail = %rb.detail, "read-back mismatch, re-applying");
            rb = win.backend.apply(&policy)?;
            if !rb.matches(&policy) {
                return Err(unavailable(format!("read-back mismatch: {}", rb.detail)));
            }
        }
        Ok(rb)
    }

    fn show_core(&self, label: &str, win: &Win) -> Result<()> {
        let policy = *win.policy.lock();
        let r = (|| {
            self.apply_core(win)?;
            win.backend.show(&policy)?;
            // show can reset level / flags (spike finding): re-assert and verify again.
            self.apply_core(win).inspect_err(|_| win.backend.hide())
        })();
        let ok = r.is_ok();
        if !ok {
            win.backend.hide();
        }
        win.visible.store(ok, Ordering::SeqCst);
        self.record(label, &r);
        r.map(|_| ())
    }

    fn reassert_core(&self, label: &str, win: &Win) {
        let policy = *win.policy.lock();
        let drift = !win.backend.verify(&policy).matches(&policy);
        let r = self.apply_core(win);
        if drift {
            tracing::warn!(label, "stealth flag drifted, re-applied");
        }
        if r.is_err() && policy.is_stealth() {
            win.backend.hide();
            win.visible.store(false, Ordering::SeqCst);
        }
        self.record(label, &r);
    }

    fn record(&self, label: &str, r: &Result<Readback>) {
        let st = match r {
            Ok(_) => StealthStatus::Verified,
            Err(e) => StealthStatus::Unavailable(e.detail.clone().unwrap_or_else(|| e.code.to_string())),
        };
        self.inner.status.lock().insert(label.to_string(), st);
        let agg = self.status();
        let mut last = self.inner.notifier.last.lock();
        if last.as_ref() != Some(&agg) {
            *last = Some(agg.clone());
            if let Some(s) = self.inner.notifier.sink.lock().clone() {
                s.emit(AppEvent::Stealth(agg));
            }
        }
    }
}

fn verify_core(win: &Win) -> StealthStatus {
    let policy = *win.policy.lock();
    let rb = win.backend.verify(&policy);
    if rb.matches(&policy) {
        StealthStatus::Verified
    } else {
        StealthStatus::Degraded(format!("flag drifted: {}", rb.detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use lowk_types::fakes::{CollectEventSink, InlineMainThread};

    fn guard() -> (StealthGuard, FakeBackend, FakeBackend) {
        let g = StealthGuard::new(Arc::new(InlineMainThread));
        let (o, m) = (FakeBackend::new(), FakeBackend::new());
        g.protect_backend("overlay", WindowPolicy::OVERLAY, Arc::new(o.clone()))
            .unwrap();
        g.protect_backend("main", WindowPolicy::main(false), Arc::new(m.clone()))
            .unwrap();
        (g, o, m)
    }

    #[test]
    fn flags_applied_before_first_show_and_independent() {
        let (g, o, m) = guard();
        assert!(o.0.lock().excluded);
        assert!(!m.0.lock().excluded);
        g.show("overlay").unwrap();
        let calls = o.calls();
        let show = calls.iter().position(|c| c == "show").unwrap();
        let first_apply = calls.iter().position(|c| c == "apply").unwrap();
        assert!(first_apply < show, "apply must precede show: {calls:?}");
        assert_eq!(g.verify("overlay"), StealthStatus::Verified);
        // main policy flip does not touch the overlay
        g.set_policy("main", WindowPolicy::main(true)).unwrap();
        assert!(m.0.lock().excluded && o.0.lock().excluded);
        g.set_policy("main", WindowPolicy::main(false)).unwrap();
        assert!(!m.0.lock().excluded && o.0.lock().excluded);
    }

    #[test]
    fn fails_closed_when_os_ignores_flag() {
        let g = StealthGuard::new(Arc::new(InlineMainThread));
        let o = FakeBackend::new();
        o.0.lock().refuse_flag = true;
        let r = g.protect_backend("overlay", WindowPolicy::OVERLAY, Arc::new(o.clone()));
        assert_eq!(r.unwrap_err().code, ErrorCode::StealthUnavailable);
        assert!(matches!(g.status(), StealthStatus::Unavailable(_)));
        let r = g.show("overlay");
        assert_eq!(r.unwrap_err().code, ErrorCode::StealthUnavailable);
        assert!(!o.0.lock().visible, "must never be visible");
    }

    #[test]
    fn show_reset_by_os_is_caught_post_show() {
        let g = StealthGuard::new(Arc::new(InlineMainThread));
        let o = FakeBackend::new();
        g.protect_backend("overlay", WindowPolicy::OVERLAY, Arc::new(o.clone()))
            .unwrap();
        o.0.lock().reset_on_show = true;
        g.show("overlay").unwrap();
        assert!(o.0.lock().excluded, "re-applied after show");
        assert!(o.0.lock().visible);
    }

    #[test]
    fn watchdog_repairs_drift_and_hides_on_unrecoverable() {
        let (g, o, _m) = guard();
        g.show("overlay").unwrap();
        o.0.lock().excluded = false; // drift
        assert!(matches!(g.verify("overlay"), StealthStatus::Degraded(_)));
        g.watchdog_tick();
        assert!(o.0.lock().excluded);
        assert!(o.0.lock().visible);
        // now the OS refuses: fail closed
        {
            let mut s = o.0.lock();
            s.excluded = false;
            s.refuse_flag = true;
        }
        g.watchdog_tick();
        assert!(!o.0.lock().visible);
        assert!(matches!(g.status(), StealthStatus::Unavailable(_)));
    }

    #[test]
    fn panic_hides_blocks_show_and_restores() {
        let (g, o, m) = guard();
        g.show("overlay").unwrap();
        g.show("main").unwrap();
        g.panic_hide();
        assert!(!o.0.lock().visible);
        assert!(m.0.lock().visible, "panic does not touch main");
        g.show("overlay").unwrap();
        assert!(!o.0.lock().visible, "re-show blocked");
        g.panic_hide();
        assert!(o.0.lock().visible, "restored via apply->verify->show");
        assert!(!g.panic_active());
    }

    #[test]
    fn status_events_deduplicated() {
        let (g, _o, _m) = guard();
        let sink = Arc::new(CollectEventSink::default());
        g.set_sink(sink.clone());
        g.show("overlay").unwrap();
        g.apply("overlay").unwrap();
        let n = sink.0.lock().unwrap().len();
        assert!(n <= 1, "{n}");
    }

    #[test]
    fn unknown_label_is_error() {
        let (g, ..) = guard();
        assert!(g.show("nope").is_err());
        assert!(matches!(g.verify("nope"), StealthStatus::Unavailable(_)));
    }
}
