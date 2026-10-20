//! Bind events -> app actions. One thread reads the merged `BindEvent` stream
//! of `lowk-input` (plugin combos + low-level hooks) and dispatches.
use crate::state::AppState;
use lowk_types::*;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

pub fn spawn(st: Arc<AppState>, rx: Receiver<BindEvent>) {
    std::thread::Builder::new()
        .name("lowk-actions".into())
        .spawn(move || {
            for ev in rx {
                st.emit(AppEvent::Bind(ev));
                dispatch(&st, ev);
            }
        })
        .expect("spawn actions thread");
}

fn dispatch(st: &Arc<AppState>, ev: BindEvent) {
    let down = ev.phase == Phase::Down;
    let r: Result<()> = (|| {
        match ev.action {
            ActionId::MoveHold => {
                let stealth = st.stealth()?;
                let cursor = st.input.lock().cursor();
                stealth.controller.move_hold(ev.phase, cursor);
                if !down {
                    crate::commands::stealth::remember_position(st);
                }
            }
            _ if !down => {}
            ActionId::ToggleRecord => crate::commands::session::toggle_record(st)?,
            ActionId::Ask => {
                let template_id = st.template.lock().clone();
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
            // G7: every monitor into the queue. The region variant follows with M3.
            ActionId::Screenshot | ActionId::ScreenshotArea => {
                crate::commands::capture::capture_into_queue(st).map(|_| ())?
            }
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
        }
    }
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
