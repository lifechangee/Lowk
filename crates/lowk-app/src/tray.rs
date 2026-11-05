//! System tray renderer for the shared indicator state.
use crate::{area, indicator, state::Shared, windows};
use lowk_types::{AttentionReason, Indicator, SessionPhase};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
#[cfg(windows)]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
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
    #[cfg(windows)]
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("default window icon is missing")?;
    #[cfg(target_os = "macos")]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))?;
    #[cfg(windows)]
    let handle = app.handle().clone();
    let state_for_menu = state.clone();
    let builder = TrayIconBuilder::with_id("lowk")
        .icon(icon)
        .tooltip(tooltip(indicator::current(), ru))
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
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
        });
    #[cfg(target_os = "macos")]
    let builder = builder.icon_as_template(true);
    #[cfg(windows)]
    let builder = builder.on_tray_icon_event(move |_, event| {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            windows::open_main(&handle);
        }
    });
    let tray = builder.build(app)?;

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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn template_icon_has_expected_dimensions_and_transparency() {
        let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))
            .expect("template PNG should decode");
        assert_eq!((icon.width(), icon.height()), (36, 36));
        assert_eq!(icon.rgba()[3], 0);
        assert!(icon.rgba().chunks_exact(4).any(|pixel| pixel[3] > 0));
        assert!(icon.rgba().chunks_exact(4).all(|pixel| pixel[..3] == [0, 0, 0]));
    }
}
