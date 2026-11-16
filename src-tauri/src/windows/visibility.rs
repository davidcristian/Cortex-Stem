//! Showing and hiding the dock windows: the auto-hide reports, the
//! toggle, and the reveal for a second instance.

use std::collections::HashMap;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use super::lookup::{dock_windows, monitor_id_of};
use super::panel::sync_dock_panel;
use super::{DOCK_LABEL, SETTINGS_LABEL};

/// Whether each dock window's panel is slid out by auto-hide, as
/// its UI last reported through `set_dock_auto_hidden`, keyed by window
/// label. `false` for a window that never reported, and again when the
/// window is created. Kept in Tauri's managed state so [`toggle_dock`] can
/// tell "bring the panel back" from "hide the window".
#[derive(Debug, Default)]
pub struct DockAutoHidden(pub(super) Mutex<HashMap<String, bool>>);

/// The dock UI of `window` reports that auto-hide slid its panel out
/// (`hidden`), or brought it back (`docs/ipc/toggle-while-hidden.md`).
/// Remembered per window label for [`toggle_dock`]; logged on a
/// change.
pub fn set_auto_hidden(
    app: &AppHandle,
    window: &WebviewWindow,
    hidden: bool,
) -> Result<(), String> {
    let flags = app
        .try_state::<DockAutoHidden>()
        .ok_or_else(|| "auto-hide state not available".to_owned())?;
    let previous = flags.0.lock().insert(window.label().to_owned(), hidden);
    if previous != Some(hidden) {
        log::debug!(
            "dock panel of {:?} {}",
            window.label(),
            if hidden { "auto-hidden" } else { "shown" }
        );
        crate::backdrop::on_auto_hidden(app, window, hidden);
    }
    Ok(())
}

/// What the UI of the dock window `label` last reported through
/// [`set_auto_hidden`]; `false` until it reports.
pub(super) fn is_auto_hidden(app: &AppHandle, label: &str) -> bool {
    app.try_state::<DockAutoHidden>()
        .is_some_and(|flags| flags.0.lock().get(label).copied().unwrap_or(false))
}

/// What one press of the toggle does to the whole set of dock windows,
/// decided from the primary's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Toggle {
    /// The primary window is visible with its panel shown: hide every window.
    Hide,
    /// The primary window is hidden: show every window.
    Show,
    /// The primary window is visible with its panel slid out by auto-hide:
    /// ask every page for its panel (and show a window that is hidden).
    Reveal,
}

/// Tray left click and the global toggle shortcut: one press brings
/// the dock back whatever hid it, the next hides it. With one dock
/// per monitor the primary's window decides and every dock window
/// follows, so one press acts on the whole set:
/// - primary hidden: show every window; the engine gets their panels back.
/// - primary visible but its panel slid out by auto-hide
///   ([`DockAutoHidden`]): ask every page to bring its panel back
///   (`dock://show`; a page whose panel is up ignores it); a secondary
///   window that happens to be hidden is shown instead.
/// - primary visible and its panel shown: hide every window; the engine has
///   none of their panels on screen meanwhile.
///
/// The UI reports its phase after the fact, so a press that lands while the
/// panel is sliding out may still hide the windows; the next press shows
/// them (and the pages, visible again, bring their panels back).
pub fn toggle_dock(app: &AppHandle) {
    let Some(primary) = app.get_webview_window(DOCK_LABEL) else {
        log::warn!("toggle_dock: no dock window");
        return;
    };
    let action = match primary.is_visible() {
        Ok(true) if is_auto_hidden(app, DOCK_LABEL) => Toggle::Reveal,
        Ok(true) => Toggle::Hide,
        Ok(false) => Toggle::Show,
        Err(e) => {
            log::warn!("toggle_dock failed: {e}");
            return;
        }
    };
    log::debug!("toggle_dock: {action:?}");
    if action == Toggle::Hide {
        // The backdrop goes before the windows: no material without its panel.
        crate::backdrop::on_panel(app, &primary, None);
    }
    for window in dock_windows(app) {
        let label = window.label().to_owned();
        let result = match action {
            Toggle::Hide => window.hide().inspect(|()| {
                log::debug!("dock window {label:?} hidden");
                if let Some(service) = app.try_state::<crate::AppService>() {
                    service
                        .0
                        .set_dock_panel(&label, monitor_id_of(app, &label), None);
                }
                // A hidden dock reserves no screen space.
                crate::appbar::sync(app, &window);
            }),
            Toggle::Show => show_dock_window(&window),
            Toggle::Reveal if window.is_visible().unwrap_or(true) => {
                log::debug!("dock reveal requested for {label:?}");
                app.emit_to(&label, crate::events::SHOW, ())
            }
            Toggle::Reveal => show_dock_window(&window),
        };
        if let Err(e) = result {
            log::warn!("toggle_dock on {label:?} failed: {e}");
        }
    }
}

/// Show a hidden dock window, give the engine its panel back and ask the
/// shell for its band again.
pub(super) fn show_dock_window(window: &WebviewWindow) -> tauri::Result<()> {
    window.show().inspect(|()| {
        log::debug!("dock window {:?} shown", window.label());
        sync_dock_panel(window, None);
        crate::appbar::sync(window.app_handle(), window);
    })
}

/// Show every dock window and bring the settings window to the front when
/// it is open. Used when a second instance is started. A dock window that
/// is on screen with its panel slid out by auto-hide is asked to bring the
/// panel back instead (`dock://show`, as [`toggle_dock`] does). No
/// dock window is focused: the docks never take the foreground on their
/// own, and a dock given the keyboard outside a focus-shortcut
/// session would have nothing to give it back to; the settings
/// window is the only one that takes the foreground, whatever the order of
/// Tauri's window map. Without it the app the user was in keeps the
/// keyboard.
pub fn reveal_all(app: &AppHandle) {
    for window in dock_windows(app) {
        let label = window.label();
        if window.is_visible().unwrap_or(false) && is_auto_hidden(app, label) {
            log::debug!("dock reveal requested by a second instance for {label:?}");
            if let Err(e) = app.emit_to(label, crate::events::SHOW, ()) {
                log::warn!("reveal {label}: {e}");
            }
            continue;
        }
        if let Err(e) = show_dock_window(&window) {
            log::warn!("reveal {label}: {e}");
        }
    }
    if let Some(settings) = app.get_webview_window(SETTINGS_LABEL) {
        let shown = settings
            .unminimize()
            .and_then(|()| settings.show())
            .and_then(|()| settings.set_focus());
        match shown {
            Ok(()) => log::debug!("settings window brought to the front by a second instance"),
            Err(e) => log::warn!("reveal {SETTINGS_LABEL}: {e}"),
        }
    }
}
