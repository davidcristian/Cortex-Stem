//! Tray icon and menu. The tray is the app's only "chrome": the
//! dock window has no close button and the app keeps running without a
//! visible window.
//!
//! - left click: show / hide the dock window;
//! - menu: *Settings…* (opens the settings window), *Restart*, *Quit*.

use tauri::AppHandle;
use tauri::image::Image;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub const TRAY_ID: &str = "dock";
const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");

const MENU_SETTINGS: &str = "settings";
const MENU_RESTART: &str = "restart";
const MENU_QUIT: &str = "quit";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, MENU_SETTINGS, "Settings…", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, MENU_RESTART, "Restart", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings, &restart, &separator, &quit])?;

    let icon = match Image::from_bytes(TRAY_PNG) {
        Ok(icon) => Some(icon),
        Err(e) => {
            log::error!("tray icon PNG is invalid ({e}); using the app icon");
            app.default_window_icon().cloned()
        }
    };

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Stem")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(on_tray_icon_event);
    if let Some(icon) = icon {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    log::debug!("tray icon created");
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().0.as_str() {
        MENU_SETTINGS => {
            if let Err(e) = crate::windows::open_settings_window(app) {
                log::warn!("opening the settings window from the tray failed: {e}");
            }
        }
        MENU_RESTART => {
            // `request_restart` goes through `ExitRequested`/`Exit`, so the
            // service shuts the platform down (hooks, events thread) before
            // the new process starts; `restart()` on the main thread skips
            // those events.
            log::info!("restart requested from the tray");
            app.request_restart();
        }
        MENU_QUIT => {
            log::info!("quit requested from the tray");
            crate::request_quit(app);
        }
        other => log::debug!("unhandled menu item {other:?}"),
    }
}

fn on_tray_icon_event(tray: &TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        crate::windows::toggle_dock(tray.app_handle());
    }
}
