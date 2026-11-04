//! Windows system tray renderer for the shared indicator state.
use crate::{area, indicator, state::Shared, windows};
use lowk_types::{AttentionReason, Indicator, SessionPhase};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::App;

const OPEN_ID: &str = "open-lowk";
const QUIT_ID: &str = "quit-lowk";

fn tooltip(state: Indicator, ru: bool) -> &'static str {
    match (state, ru) {
        (Indicator::Background, true) => "Lowk работает в фоне · запись не идёт",
        (Indicator::Background, false) => "Lowk is running in the background · not recording",
        (
            Indicator::Attention {
                reason: AttentionReason::EngineMissing,
            },
            true,
        ) => "Lowk: нужно скачать модель распознавания",
        (
            Indicator::Attention {
                reason: AttentionReason::EngineMissing,
            },
            false,
        ) => "Lowk: the speech model needs downloading",
        (Indicator::Hidden, _) => "Lowk",
    }
}

pub fn setup(app: &mut App, state: Shared) -> Result<(), Box<dyn std::error::Error>> {
    let ru = area::lang(&state) == "ru";
    let open = MenuItem::with_id(
        app,
        OPEN_ID,
        if ru { "Открыть Lowk" } else { "Open Lowk" },
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        QUIT_ID,
        if ru { "Выйти" } else { "Quit" },
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("default window icon is missing")?;
    let handle = app.handle().clone();
    let state_for_menu = state.clone();
    let tray = TrayIconBuilder::with_id("lowk")
        .icon(icon)
        .tooltip(tooltip(indicator::current(), ru))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            OPEN_ID => windows::open_main(app),
            QUIT_ID => {
                if state_for_menu.session.state().phase == SessionPhase::Idle {
                    app.exit(0);
                } else {
                    tracing::warn!(phase = ?state_for_menu.session.state().phase, "ignoring tray quit request while a session is active");
                }
            }
            _ => {}
        })
        .on_tray_icon_event(move |_, event| {
            if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
                windows::open_main(&handle);
            }
        })
        .build(app)?;

    let tray = tray.clone();
    let state_for_render = state.clone();
    indicator::subscribe(Box::new(move |indicator| {
        if indicator == Indicator::Hidden {
            let _ = tray.set_visible(false);
        } else {
            let ru = area::lang(&state_for_render) == "ru";
            let _ = tray.set_tooltip(Some(tooltip(indicator, ru)));
            let _ = tray.set_visible(true);
        }
    }));
    Ok(())
}
