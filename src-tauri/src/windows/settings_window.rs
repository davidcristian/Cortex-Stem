//! The settings window: created on demand, hidden when it is asked to
//! close and destroyed [`SETTINGS_CLOSE_GRACE`] later, and its title bar's
//! theme.

use parking_lot::Mutex;
use stem_core::settings::ThemeMode;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use super::SETTINGS_LABEL;

const SETTINGS_SIZE: (f64, f64) = (760.0, 600.0);
const SETTINGS_MIN_SIZE: (f64, f64) = (560.0, 420.0);

/// `settings.theme` as the settings window's title bar last took it (or
/// will take it, while the window is closed), so a settings event only acts
/// on a change. Kept in Tauri's managed state.
#[derive(Debug, Default)]
pub struct SettingsWindowTheme(Mutex<Option<ThemeMode>>);

/// `settings.theme` changed: the settings window's title bar follows it (the
/// page itself follows `dock://settings`). Any thread; never blocks.
pub(super) fn on_theme_setting(app: &AppHandle, mode: ThemeMode) {
    let Some(state) = app.try_state::<SettingsWindowTheme>() else {
        return;
    };
    {
        let mut current = state.0.lock();
        if *current == Some(mode) {
            return;
        }
        *current = Some(mode);
    }
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply_settings_window_theme(&handle, mode)) {
        log::warn!("could not schedule the settings window's theme change: {e}");
    }
}

/// Give the open settings window's title bar the theme for `mode`. A closed
/// window takes the setting when it is created. Main thread.
fn apply_settings_window_theme(app: &AppHandle, mode: ThemeMode) {
    let Some(window) = app.get_webview_window(SETTINGS_LABEL) else {
        log::debug!("theme setting {mode:?}: the settings window is not open");
        return;
    };
    match window.set_theme(native_theme(mode)) {
        Ok(()) => log::debug!("settings window title bar: {mode:?}"),
        Err(e) => log::warn!("the settings window's theme could not be set: {e}"),
    }
}

/// `settings.theme` → the native window theme: forced light or dark, or
/// `None` to follow Windows. Pure.
pub fn native_theme(mode: ThemeMode) -> Option<tauri::Theme> {
    match mode {
        ThemeMode::System => None,
        ThemeMode::Light => Some(tauri::Theme::Light),
        ThemeMode::Dark => Some(tauri::Theme::Dark),
    }
}

/// How long a settings window that was asked to close stays, hidden,
/// before it is destroyed: time for its page to send an edit it still held
/// back (a debounced change, a typed value its blur commits) on
/// `dock://settings-closing`.
const SETTINGS_CLOSE_GRACE: std::time::Duration = std::time::Duration::from_millis(250);

/// Open the settings window, or show and focus it if it already exists.
/// Closing it destroys it ([`on_settings_close_requested`]); the app keeps
/// running. A new window's title bar takes the current *Theme* setting.
/// Main thread.
pub fn open_settings_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        window.show()?;
        window.set_focus()?;
        log::debug!("settings window focused");
        return Ok(());
    }
    let mode = app
        .try_state::<crate::AppService>()
        .map(|service| service.0.settings().theme);
    if let (Some(mode), Some(state)) = (mode, app.try_state::<SettingsWindowTheme>()) {
        *state.0.lock() = Some(mode);
    }
    let window =
        WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings.html".into()))
            .title("Stem Settings")
            .theme(mode.and_then(native_theme))
            .inner_size(SETTINGS_SIZE.0, SETTINGS_SIZE.1)
            .min_inner_size(SETTINGS_MIN_SIZE.0, SETTINGS_MIN_SIZE.1)
            .resizable(true)
            .decorations(true)
            .always_on_top(false)
            .center()
            .focused(true)
            .build()?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            on_settings_close_requested(&handle, api);
        }
    });
    log::info!("settings window opened");
    Ok(())
}

/// The settings window was asked to close (×, Alt+F4). Destroying it at
/// once would drop what its page still holds back: an edit waits 150 ms
/// before it is sent, and a typed value is committed only when its field
/// loses the focus. So the window is hidden at once (it looks closed),
/// its page is told (`dock://settings-closing`: it commits the focused
/// field and sends any pending edit), and it is destroyed
/// [`SETTINGS_CLOSE_GRACE`] later, unless it was opened again meanwhile.
/// A window that cannot be hidden, or that is already hidden, closes at
/// once as before. Main thread (the window's event).
fn on_settings_close_requested(app: &AppHandle, api: &tauri::CloseRequestApi) {
    let Some(window) = app.get_webview_window(SETTINGS_LABEL) else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    if let Err(e) = window.hide() {
        log::warn!(
            "the settings window could not be hidden before it closes ({e}); closing it at once"
        );
        return;
    }
    api.prevent_close();
    if let Err(e) = app.emit_to(SETTINGS_LABEL, crate::events::SETTINGS_CLOSING, ()) {
        log::warn!("telling the settings page it closes failed: {e}");
    }
    let handle = app.clone();
    drop(tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(SETTINGS_CLOSE_GRACE);
        let main = handle.clone();
        let scheduled = handle.run_on_main_thread(move || {
            let Some(window) = main.get_webview_window(SETTINGS_LABEL) else {
                return;
            };
            if window.is_visible().unwrap_or(false) {
                log::debug!("settings window opened again before it closed; kept");
                return;
            }
            match window.destroy() {
                Ok(()) => log::info!("settings window closed"),
                Err(e) => log::warn!("the settings window could not be closed: {e}"),
            }
        });
        if let Err(e) = scheduled {
            log::warn!("could not schedule closing the settings window: {e}");
        }
    }));
}
