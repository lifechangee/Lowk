//! Cross-platform indicator state. The macOS menu bar renderer will subscribe here later.
use lowk_types::{AttentionReason, Indicator, SessionPhase};
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter};

type Subscriber = Arc<dyn Fn(Indicator) + Send + Sync>;

static CURRENT: OnceLock<Mutex<Option<Indicator>>> = OnceLock::new();
static SUBSCRIBERS: OnceLock<Mutex<Vec<Subscriber>>> = OnceLock::new();

fn current_slot() -> &'static Mutex<Option<Indicator>> {
    CURRENT.get_or_init(|| Mutex::new(None))
}

fn subscribers() -> &'static Mutex<Vec<Subscriber>> {
    SUBSCRIBERS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn derive(phase: SessionPhase, engine_ready: bool) -> Indicator {
    if phase != SessionPhase::Idle {
        Indicator::Hidden
    } else if !engine_ready {
        Indicator::Attention {
            reason: AttentionReason::EngineMissing,
        }
    } else {
        Indicator::Background
    }
}

fn changed(prev: Option<Indicator>, next: Indicator) -> Option<Indicator> {
    (prev != Some(next)).then_some(next)
}

/// Registers a platform renderer. A renderer registered after initialization receives current state.
/// The macOS menu bar renderer will be wired here by the other team; this module does not implement it.
// Only the Windows tray calls it so far; the macOS menu bar will use it (drop this allow then).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn subscribe(f: Box<dyn Fn(Indicator) + Send + Sync>) {
    let current = *current_slot().lock();
    let f: Subscriber = Arc::from(f);
    subscribers().lock().push(f.clone());
    if let Some(current) = current {
        f(current);
    }
}

/// Recomputes from the live state: after an engine install or a settings change (engine, key) the
/// «model needed» attention must clear without waiting for the next call.
pub fn refresh(st: &crate::state::AppState) {
    recompute(&st.app, st.session.state().phase, st.engine_ready());
}

pub fn current() -> Indicator {
    current_slot().lock().unwrap_or(Indicator::Hidden)
}

pub fn recompute(app: &AppHandle, phase: SessionPhase, engine_ready: bool) {
    let next = derive(phase, engine_ready);
    let emitted = {
        let mut current = current_slot().lock();
        let Some(next) = changed(*current, next) else {
            return;
        };
        let previous = current.replace(next);
        (previous, next)
    };

    tracing::info!(from = ?emitted.0, to = ?emitted.1, "indicator changed");
    let _ = app.emit(lowk_types::events::INDICATOR_STATE, emitted.1);
    let subscribers = subscribers().lock().clone();
    for subscriber in subscribers {
        subscriber(emitted.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_covers_each_phase_and_readiness() {
        for ready in [false, true] {
            assert_eq!(
                derive(SessionPhase::Idle, ready),
                if ready {
                    Indicator::Background
                } else {
                    Indicator::Attention {
                        reason: AttentionReason::EngineMissing,
                    }
                }
            );
            for phase in [
                SessionPhase::Starting,
                SessionPhase::Recording,
                SessionPhase::Paused,
                SessionPhase::Stopping,
            ] {
                assert_eq!(derive(phase, ready), Indicator::Hidden);
            }
        }
    }

    #[test]
    fn changed_only_returns_new_state() {
        let background = Indicator::Background;
        assert_eq!(changed(None, background), Some(background));
        assert_eq!(changed(Some(background), background), None);
        assert_eq!(
            changed(Some(background), Indicator::Hidden),
            Some(Indicator::Hidden)
        );
    }
}
