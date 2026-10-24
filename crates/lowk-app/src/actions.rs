//! Bind events -> app actions. One thread reads the merged `BindEvent` stream
//! of `lowk-input` (plugin combos + low-level hooks) and dispatches.
use crate::state::AppState;
use lowk_types::*;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

pub fn spawn(st: Arc<AppState>, rx: Receiver<BindEvent>) {
    std::thread::Builder::new()
        .name("lowk-actions".into())
        .spawn(move || {
            for ev in rx {
                st.emit(AppEvent::Bind(ev.clone()));
                dispatch(&st, ev);
            }
        })
        .expect("spawn actions thread");
}

/// Ctrl must stay down this long before the overlay turns clickable (a Ctrl+C is not a hold).
const HOVER_DELAY: std::time::Duration = std::time::Duration::from_millis(140);

pub(crate) fn dispatch(st: &Arc<AppState>, ev: BindEvent) {
    let down = ev.phase == Phase::Down;
    // Action names only (never key content): proves in lowk.log that a press reached the app.
    tracing::info!(action = ?ev.action, phase = ?ev.phase, "bind event");
    let r: Result<()> = (|| {
        match ev.action {
            ActionId::MoveHold => {
                let stealth = st.stealth()?;
                let cursor = st.input.lock().cursor();
                // The overlay when it is up, else the pill (G2). One magnet at a time.
                let to_pill = st.pill_moving.load(Ordering::SeqCst) == 1
                    || (!stealth.guard.is_visible(lowk_stealth::OVERLAY) && pill_up(stealth));
                if !to_pill {
                    stealth.controller.move_hold(ev.phase, cursor);
                    if !down {
                        crate::commands::stealth::remember_position(st);
                    }
                } else if let Some(p) = &stealth.pill {
                    if down {
                        st.pill_moving.store(1, Ordering::SeqCst);
                        p.move_hold(Phase::Down, cursor);
                    } else if st.pill_moving.swap(0, Ordering::SeqCst) == 1 {
                        p.move_hold(Phase::Up, cursor);
                        crate::commands::stealth::remember_pill_position(st);
                    }
                }
            }
            ActionId::ClickHold => {
                let ctl = &st.stealth()?.overlay;
                let gen = st.hover_gen.fetch_add(1, Ordering::SeqCst) + 1;
                if down {
                    let st2 = st.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(HOVER_DELAY);
                        if st2.hover_gen.load(Ordering::SeqCst) == gen {
                            if let Ok(s) = st2.stealth() {
                                s.overlay.hover(Phase::Down);
                            }
                        }
                    });
                } else {
                    ctl.hover(Phase::Up);
                }
            }
            _ if !down => {}
            ActionId::ToggleRecord => crate::commands::session::toggle_record(st)?,
            ActionId::ToggleMic => crate::commands::session::toggle_mic(st)?,
            ActionId::Ask => {
                let template_id = st.template.lock().clone();
                st.session.ask(AskRequest {
                    template_id,
                    typed_text: None,
                })?;
            }
            ActionId::AskBrief
            | ActionId::AskComplement
            | ActionId::AskSay
            | ActionId::AskFactcheck
            | ActionId::AskFollowup
            | ActionId::AskRecap => {
                // G6: a fixed template per bind, no overlay mode switch needed.
                let template_id = ev.action.template_id().unwrap_or("brief").to_string();
                st.session.ask(AskRequest {
                    template_id,
                    typed_text: None,
                })?;
            }
            ActionId::AskTemplate => {
                // A bind of any template (built-in or user-created): the id travels with the bind.
                let Some(template_id) = ev.template.clone() else {
                    return Err(AppError::new(ErrorCode::InvalidArgument)
                        .with_detail("ask_template bind without a template"));
                };
                if !st.settings.get().templates.iter().any(|t| t.id == template_id) {
                    // The template was deleted after it got a bind: say so instead of asking with a wrong one.
                    return Err(AppError::new(ErrorCode::NotFound).with_detail("template of the bind"));
                }
                st.session.ask(AskRequest {
                    template_id,
                    typed_text: None,
                })?;
            }
            ActionId::ToggleInteractive => {
                let ctl = &st.stealth()?.controller;
                let next = if ctl.mode() == OverlayMode::Interactive {
                    OverlayMode::Passive
                } else {
                    OverlayMode::Interactive
                };
                ctl.set_mode(next)?;
            }
            ActionId::ToggleOverlay => crate::visibility::toggle(st)?,
            ActionId::PanicHide => st.stealth()?.controller.panic_hide(),
            ActionId::ToggleMain => crate::windows::open_main(&st.app),
            // G7: the active monitor (per `capture.monitors`) into the queue.
            ActionId::Screenshot => crate::commands::capture::capture_into_queue(st).map(|_| ())?,
            // Crosshair selection over the display under the pointer; the grab joins the same queue.
            ActionId::ScreenshotArea => crate::area::open(st)?,
            ActionId::NextTemplate | ActionId::PrevTemplate => {
                cycle_template(st, ev.action == ActionId::NextTemplate)
            }
            ActionId::ClearShots => crate::commands::capture::clear_queue(st),
            ActionId::CancelAnswer => {}
        }
        Ok(())
    })();
    if let Err(e) = r {
        // SessionIdle for Ask outside a recording is normal; everything else is worth a log line.
        if e.code != ErrorCode::SessionIdle {
            tracing::warn!(action = ?ev.action, %e, "bind action failed");
            // A key that does nothing and says nothing reads as «hotkeys are dead»: tell the windows
            // why (a missing permission) so the overlay / main window can say it.
            let perm = match e.code {
                ErrorCode::PermScreenRecording => Some("screen_recording"),
                ErrorCode::PermInputMonitoring => Some("input_monitoring"),
                ErrorCode::PermAccessibility => Some("accessibility"),
                ErrorCode::PermMicrophone => Some("microphone"),
                ErrorCode::PermSystemAudio => Some("system_audio"),
                _ => None,
            };
            if let Some(perm) = perm {
                st.emitter.broadcast(
                    "binds://failed",
                    serde_json::json!({ "action": ev.action, "perm": perm }),
                );
            }
        }
    }
}

fn pill_up(s: &lowk_stealth::Stealth) -> bool {
    s.pill.as_ref().is_some_and(|p| p.is_visible())
}

fn cycle_template(st: &AppState, forward: bool) {
    let ts = st.settings.get().templates;
    let visible: Vec<&Template> = ts.iter().filter(|t| t.id != TEMPLATE_SUMMARY).collect();
    if visible.is_empty() {
        return;
    }
    let mut cur = st.template.lock();
    let i = visible.iter().position(|t| t.id == *cur).unwrap_or(0);
    let n = visible.len();
    let j = if forward { (i + 1) % n } else { (i + n - 1) % n };
    *cur = visible[j].id.clone();
}
