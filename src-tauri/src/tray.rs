//! Tray icon and menu. The tray is the app's only "chrome":
//! the dock window has no close button and the app keeps running without a
//! visible window.
//!
//! - left click: show / hide the dock window (`behavior.toggleShortcut`
//!   does the same from the keyboard, see `shortcut.rs`);
//! - menu: *Settings…* (opens the settings window), *Click through outside
//!   the dock* (a check item bound to `behavior.clickThrough`), *Restart*,
//!   *Quit*.
//!
//! The check item lives in Tauri's managed state ([`TrayMenu`]) so that a
//! settings change from anywhere (the settings window, a command) updates
//! its check mark through [`on_settings_changed`]. Clicking it only asks the
//! service to flip the setting; the settings event that follows is what
//! turns click-through on or off (`windows::on_settings_changed`), the same
//! path every other source of the change takes.

use std::sync::Arc;

use stem_core::Settings;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::AppService;

pub const TRAY_ID: &str = "stem";
const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");

const MENU_SETTINGS: &str = "settings";
const MENU_CLICK_THROUGH: &str = "click-through";
const MENU_RESTART: &str = "restart";
const MENU_QUIT: &str = "quit";

/// The tray's stateful entries.
pub struct TrayMenu {
    click_through: CheckMenuItem<Wry>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let click_through_on = app
        .try_state::<AppService>()
        .is_none_or(|service| service.0.settings().behavior.click_through);
    let settings = MenuItem::with_id(app, MENU_SETTINGS, "Settings…", true, None::<&str>)?;
    let click_through = CheckMenuItem::with_id(
        app,
        MENU_CLICK_THROUGH,
        "Click through outside the dock",
        true,
        click_through_on,
        None::<&str>,
    )?;
    let restart = MenuItem::with_id(app, MENU_RESTART, "Restart", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&settings, &click_through, &restart, &separator, &quit],
    )?;

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
    app.manage(TrayMenu { click_through });
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
        MENU_CLICK_THROUGH => toggle_click_through(app),
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

/// Flip `behavior.clickThrough` through the service, off the main thread
/// (the service persists the settings file). The menu library has already
/// toggled the check mark on the click; a failed save puts it back.
fn toggle_click_through(app: &AppHandle) {
    let Some(service) = app.try_state::<AppService>().map(|s| Arc::clone(&s.0)) else {
        log::warn!("click-through toggle ignored: the service is not running");
        return;
    };
    let app = app.clone();
    drop(tauri::async_runtime::spawn_blocking(move || {
        let mut settings = service.settings();
        let was = settings.behavior.click_through;
        settings.behavior.click_through = !was;
        match service.update_settings(settings) {
            Ok(saved) => log::info!(
                "click-through {} from the tray",
                crate::on_off(saved.behavior.click_through)
            ),
            Err(e) => {
                log::warn!("toggling click-through from the tray failed: {e}");
                set_click_through_checked(&app, was);
            }
        }
    }));
}

/// The settings changed from any source: the check mark follows. Runs on
/// the service worker thread; the menu write hops to the main thread.
pub fn on_settings_changed(app: &AppHandle, settings: &Settings) {
    set_click_through_checked(app, settings.behavior.click_through);
}

fn set_click_through_checked(app: &AppHandle, checked: bool) {
    let Some(tray) = app.try_state::<TrayMenu>() else {
        return;
    };
    let item = tray.click_through.clone();
    // `set_checked` from another thread would hop to the main thread and
    // wait for it; hop without waiting so the service thread never blocks.
    let scheduled = app.run_on_main_thread(move || {
        if let Err(e) = item.set_checked(checked) {
            log::debug!("tray: could not update the click-through check mark: {e}");
        }
    });
    if let Err(e) = scheduled {
        log::debug!("tray: could not schedule the check-mark update: {e}");
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
