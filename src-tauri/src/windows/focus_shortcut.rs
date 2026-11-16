//! A press of the focus shortcut: the dock window it picks is brought on
//! screen and made the foreground window.

use stem_core::MonitorId;
use stem_platform::PlatformError;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use super::DOCK_LABEL;
use super::focus_session::{DockFocus, ReturnTo, end_outcome};
use super::focus_target::{
    focus_target, own_foreground, own_window_monitor, return_target, window_monitor,
};
use super::lookup::native_handle;
use super::state::DockGeometry;
use super::visibility::{is_auto_hidden, show_dock_window};

/// The focus shortcut was pressed (`docs/ipc/keyboard-entry.md`).
/// Main thread: the plugin's handler, inside the `WM_HOTKEY` that gives
/// Stem the right to take the foreground.
///
/// A press while a dock window has the keyboard from an earlier press (it
/// is in front and the session is its own, [`DockFocus::holder`]) tells
/// its page `dock://release-focus`: the page does what Escape does, lets go
/// of its item and calls `release_dock_focus`, which brings the remembered
/// window back if there is one. Otherwise, also while a dock is in front
/// without the session: remember the window in front ([`return_target`]:
/// the settings window when it is the foreground window, else the engine's
/// active window; [`DockFocus::begin`]); pick the dock window on its
/// monitor ([`focus_target`]); show that window if the tray or the toggle hid it,
/// or ask its page for the panel (`dock://show`) if auto-hide slid it out;
/// then, off the main thread, make it the foreground window
/// (`Platform::focus_dock_window`) and tell its page
/// (`dock://focus`), which focuses its toolbar ([`activated`]).
pub fn on_focus_shortcut(app: &AppHandle) {
    let (Some(service), Some(focus)) = (
        app.try_state::<crate::AppService>(),
        app.try_state::<DockFocus>(),
    ) else {
        return;
    };
    let service = std::sync::Arc::clone(&service.0);
    // Which of our windows is in front, if one is: a dock (the holder of an
    // earlier press, or one in front without the session) or the settings
    // window (the window to go back to).
    let own = own_foreground(app, service.platform().as_ref());
    let front = own.as_deref().filter(|label| label.starts_with(DOCK_LABEL));
    if let Some(label) = focus.holder(front) {
        log::debug!("focus shortcut: dock window {label:?} has the keyboard; it lets go");
        if let Err(e) = app.emit_to(&label, crate::events::RELEASE_FOCUS, ()) {
            log::warn!("focus shortcut: telling dock window {label:?} to let go failed: {e}");
        }
        return;
    }

    let state = service.state();
    let window = return_target(own.as_deref(), state.foreground);
    let monitor = match &window {
        Some(ReturnTo::App(id)) => window_monitor(&state, *id),
        Some(ReturnTo::Own(label)) => own_window_monitor(app, label, &service.monitors()),
        None => None,
    };
    let docks: Vec<(String, Option<MonitorId>)> = app
        .try_state::<DockGeometry>()
        .map(|geometry| {
            geometry
                .labels()
                .into_iter()
                .map(|label| {
                    let monitor = geometry.dock(&label).and_then(|dock| dock.monitor);
                    (label, monitor)
                })
                .collect()
        })
        .unwrap_or_default();
    let label = focus_target(service.settings().monitors, monitor.as_ref(), &docks);
    let Some(dock) = app.get_webview_window(&label) else {
        log::warn!("focus shortcut: no dock window {label:?}");
        return;
    };
    bring_on_screen(app, &dock);
    let Some(handle) = native_handle(&dock) else {
        return;
    };
    log::info!(
        "focus shortcut: the keyboard goes to dock window {label:?} (monitor {}); Escape goes back to {}",
        monitor.as_ref().map_or("unknown", |m| m.0.as_str()),
        match &window {
            Some(ReturnTo::App(id)) => format!("{id:?}"),
            Some(ReturnTo::Own(own)) => format!("our window {own:?}"),
            None => "nothing".to_owned(),
        }
    );
    focus.begin(&label, window);
    let app = app.clone();
    // Off the main thread: the activation's fallbacks touch other threads'
    // input state (docs/development/windows-rules.md). The page is told
    // once its window is in front; until the WebView has the focus the
    // browser holds the page's `focus()` back, so either order ends with
    // the item focused.
    drop(tauri::async_runtime::spawn_blocking(move || {
        let result = service.platform().focus_dock_window(handle);
        let Some(focus) = app.try_state::<DockFocus>() else {
            return;
        };
        if activated(&focus, &label, result)
            && let Err(e) = app.emit_to(&label, crate::events::FOCUS, ())
        {
            log::warn!("focus shortcut: telling dock window {label:?} failed: {e}");
        }
    }));
}

/// The activation of the dock window `label` at a press ended with
/// `result`: whether its page is to be told (`dock://focus`). A failure is
/// a warning and ends the session if it is still that dock's (a later press
/// may have given another dock the focus meanwhile); the page is not told.
pub(super) fn activated(focus: &DockFocus, label: &str, result: Result<(), PlatformError>) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            log::warn!(
                "focus shortcut: dock window {label:?} could not take the keyboard focus: {e}"
            );
            if let Some(ended) = focus.end(label) {
                log::debug!(
                    "dock window {label:?}: its activation failed; {}",
                    end_outcome(&ended)
                );
            }
            false
        }
    }
}

/// Step 3 of a press: a dock window the tray or the toggle hid is shown; one
/// whose panel auto-hide slid out gets `dock://show`, and the page's
/// `focus` hold keeps the panel up once an item has the focus.
fn bring_on_screen(app: &AppHandle, dock: &WebviewWindow) {
    let label = dock.label();
    match dock.is_visible() {
        Ok(false) => match show_dock_window(dock) {
            Ok(()) => log::debug!("focus shortcut: dock window {label:?} shown"),
            Err(e) => log::warn!("focus shortcut: showing dock window {label:?} failed: {e}"),
        },
        Ok(true) if is_auto_hidden(app, label) => {
            log::debug!("focus shortcut: dock reveal requested for {label:?}");
            if let Err(e) = app.emit_to(label, crate::events::SHOW, ()) {
                log::warn!("focus shortcut: revealing dock window {label:?} failed: {e}");
            }
        }
        Ok(true) => {}
        Err(e) => log::warn!("focus shortcut: dock window {label:?}: {e}"),
    }
}
