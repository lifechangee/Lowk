//! Overlay / pill capture visibility: one owner for every transition (the bind and the settings watcher).
//! Principle: HIDING is always safe and wins; SHOWING is deliberate and transactional (both windows or none).

use crate::guard::StealthGuard;
use crate::policy::WindowPolicy;
use crate::{OVERLAY, PILL};
use lowk_types::Result;
use parking_lot::Mutex;
use std::sync::Arc;

type Listener = Arc<dyn Fn(bool) + Send + Sync>;

pub struct CaptureSwitch {
    guard: StealthGuard,
    has_pill: bool,
    /// Serialises transitions: no stale interleaving between a bind and a queued settings snapshot.
    txn: Mutex<()>,
    /// Told the APPLIED state after every transition (the UI badge follows it, not the saved preference).
    on_capture: Mutex<Option<Listener>>,
}

impl CaptureSwitch {
    pub fn new(guard: StealthGuard, has_pill: bool) -> Self {
        Self {
            guard,
            has_pill,
            txn: Mutex::new(()),
            on_capture: Mutex::new(None),
        }
    }

    /// Callback for the applied capture state (called after every transition, under the transition lock).
    pub fn set_listener(&self, f: Arc<dyn Fn(bool) + Send + Sync>) {
        *self.on_capture.lock() = Some(f);
    }

    /// Apply to overlay and pill as ONE transaction. Hiding always wins: showing that fails on either window
    /// re-excludes both (a window that cannot be excluded is hidden by the guard). Never re-exposes on error.
    fn apply_capture(&self, visible: bool, source: &str) -> Result<()> {
        let old = self.applied();
        tracing::info!(old, new = visible, source, "overlay capture visibility changed");
        let r1 = self.guard.set_policy(OVERLAY, WindowPolicy::overlay(visible));
        let r2 = if self.has_pill {
            self.guard.set_policy(PILL, WindowPolicy::pill(visible))
        } else {
            Ok(())
        };
        let r = r1.and(r2);
        if visible && r.is_err() {
            tracing::error!(
                source,
                "enabling capture visibility failed: both windows re-excluded"
            );
            let _ = self.guard.set_policy(OVERLAY, WindowPolicy::overlay(false));
            if self.has_pill {
                let _ = self.guard.set_policy(PILL, WindowPolicy::pill(false));
            }
        }
        if let Some(f) = self.on_capture.lock().clone() {
            f(self.applied());
        }
        r
    }

    /// A user-initiated change (the bind). `persist` saves the new preference; it runs inside the transition
    /// lock so the settings watcher cannot interleave. Hide: applied first, never rolled back, a save failure is
    /// reported. Show: only kept when applied on both windows AND saved; otherwise hidden again.
    pub fn change(&self, visible: bool, source: &str, persist: &dyn Fn(bool) -> Result<()>) -> Result<()> {
        let _t = self.txn.lock();
        if visible {
            self.apply_capture(true, source)?;
            if let Err(e) = persist(true) {
                let _ = self.apply_capture(false, source);
                return Err(e);
            }
            Ok(())
        } else {
            let applied = self.apply_capture(false, source);
            let saved = persist(false);
            if let Err(e) = &saved {
                tracing::error!(%e, "overlay is hidden but the setting could not be saved (it may come back visible after a restart)");
            }
            applied.and(saved)
        }
    }

    /// Settings watcher: make the windows match the CURRENT saved value (read under the transition lock, never a
    /// queued snapshot). Returns `Some(Err)` when enabling failed (the caller should save `false`).
    pub fn reconcile(&self, current: &dyn Fn() -> bool, source: &str) -> Option<Result<()>> {
        let _t = self.txn.lock();
        let want = current();
        if want == self.applied() {
            return None;
        }
        Some(self.apply_capture(want, source))
    }

    /// The APPLIED state: both the overlay and (when present) the pill are set visible to capture.
    pub fn applied(&self) -> bool {
        self.guard.capture_excluded(OVERLAY) == Some(false)
            && (!self.has_pill || self.guard.capture_excluded(PILL) == Some(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use lowk_types::fakes::InlineMainThread;
    use lowk_types::{AppError, ErrorCode};
    use std::sync::atomic::{AtomicBool, Ordering};

    fn rig(visible: bool) -> (CaptureSwitch, StealthGuard, FakeBackend, FakeBackend) {
        let g = StealthGuard::new(Arc::new(InlineMainThread));
        let (o, p) = (FakeBackend::new(), FakeBackend::new());
        g.protect_backend(OVERLAY, WindowPolicy::overlay(visible), Arc::new(o.clone()))
            .unwrap();
        g.protect_backend(PILL, WindowPolicy::pill(visible), Arc::new(p.clone()))
            .unwrap();
        g.show(OVERLAY).unwrap();
        g.show(PILL).unwrap();
        (CaptureSwitch::new(g.clone(), true), g, o, p)
    }
    fn save_err(_: bool) -> Result<()> {
        Err(AppError::new(ErrorCode::Unknown).with_detail("disk full"))
    }

    #[test]
    fn hide_is_never_rolled_back_when_saving_fails() {
        let (c, _g, o, p) = rig(true);
        assert!(c.applied());
        let r = c.change(false, "bind", &save_err);
        assert!(r.is_err(), "the save failure is reported");
        assert!(!c.applied());
        assert!(o.0.lock().excluded && p.0.lock().excluded, "stays hidden");
        assert!(o.0.lock().visible, "window still shown, just excluded");
    }

    #[test]
    fn hide_that_cannot_exclude_hides_the_windows() {
        let (c, _g, o, p) = rig(true);
        o.0.lock().refuse_flag = true;
        let saved = AtomicBool::new(true);
        let r = c.change(false, "bind", &|v| {
            saved.store(v, Ordering::SeqCst);
            Ok(())
        });
        assert!(r.is_err());
        assert!(!o.0.lock().visible, "fail closed: overlay hidden");
        assert!(p.0.lock().excluded);
        assert!(
            !saved.load(Ordering::SeqCst),
            "the hide is still persisted after an exclusion failure"
        );
    }

    #[test]
    fn stale_visible_snapshot_never_re_exposes_after_a_hide() {
        let (c, _g, o, p) = rig(true);
        let saved = AtomicBool::new(true); // a queued snapshot still says true
        c.change(false, "bind", &|v| {
            saved.store(v, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        // the watcher now processes the OLD snapshot: it reads the current saved value, not the snapshot
        assert!(c
            .reconcile(&|| saved.load(Ordering::SeqCst), "settings")
            .is_none());
        assert!(!c.applied());
        assert!(o.0.lock().excluded && p.0.lock().excluded, "never visible again");
    }

    #[test]
    fn reconcile_applies_the_current_value_and_logs_source() {
        let (c, _g, o, _p) = rig(false);
        assert!(c.reconcile(&|| true, "settings").unwrap().is_ok());
        assert!(c.applied() && !o.0.lock().excluded);
        assert!(c.reconcile(&|| true, "settings").is_none(), "no change, no apply");
        assert!(c.reconcile(&|| false, "settings").unwrap().is_ok());
        assert!(!c.applied() && o.0.lock().excluded);
    }

    #[test]
    fn enabling_is_transactional_across_overlay_and_pill() {
        let (c, g, o, p) = rig(false);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        c.set_listener(Arc::new(move |v| s2.lock().push(v)));
        p.0.lock().refuse_flag = true; // the pill cannot be changed
        let saved = AtomicBool::new(false);
        let r = c.change(true, "bind", &|v| {
            saved.store(v, Ordering::SeqCst);
            Ok(())
        });
        assert!(r.is_err());
        assert!(o.0.lock().excluded, "overlay re-excluded, not left visible");
        assert!(p.0.lock().excluded);
        assert!(!c.applied());
        assert!(!saved.load(Ordering::SeqCst), "nothing saved");
        assert_eq!(g.capture_excluded(OVERLAY), Some(true));
        assert!(!*seen.lock().last().unwrap(), "UI told the APPLIED state");
    }

    #[test]
    fn enabling_that_cannot_be_saved_is_hidden_again() {
        let (c, _g, o, p) = rig(false);
        assert!(c.change(true, "bind", &save_err).is_err());
        assert!(!c.applied());
        assert!(o.0.lock().excluded && p.0.lock().excluded);
    }

    #[test]
    fn listener_follows_the_applied_state_both_ways() {
        let (c, ..) = rig(false);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        c.set_listener(Arc::new(move |v| s2.lock().push(v)));
        c.change(true, "bind", &|_| Ok(())).unwrap();
        c.change(false, "bind", &|_| Ok(())).unwrap();
        assert_eq!(*seen.lock(), vec![true, false]);
    }
}
