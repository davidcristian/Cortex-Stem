//! Giving the keyboard focus back, and the end of the focus shortcut's
//! session when a dock is clicked, closed or loses the activation.

use stem_core::{WindowAction, WindowId};
use stem_platform::{Platform, PlatformError};
use tauri::{AppHandle, Manager};

use super::focus_session::{DockFocus, Release, ReturnTo, end_outcome};
use super::focus_target::focused_dock;

/// `release_dock_focus` from the dock window `label` (Escape, or the page's
/// answer to a second press's `dock://release-focus`): [`give_focus_back`]
/// with the dock window that is in front now, an app's window brought back
/// through the platform and Stem's own window through [`focus_own_window`].
/// Blocking (bounded): never on the main thread.
pub fn release_dock_focus(
    app: &AppHandle,
    focus: &DockFocus,
    platform: &dyn Platform,
    label: &str,
) -> Result<(), String> {
    let front = focused_dock(app, platform);
    give_focus_back(focus, platform, label, front.as_deref(), |own| {
        focus_own_window(app, label, own)
    })
}

/// The dock window `label` gives the keyboard focus back while `front` is
/// the dock window in front: take the session ([`DockFocus::release`]) and
/// bring the remembered window back, an app's with [`return_focus`], Stem's
/// own with `focus_own`; with nothing to bring back, or a session another
/// dock still holds, the foreground stays. Blocking (bounded): never on the
/// main thread.
pub fn give_focus_back(
    focus: &DockFocus,
    platform: &dyn Platform,
    label: &str,
    front: Option<&str>,
    focus_own: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    match focus.release(label, front) {
        Release::Window(ReturnTo::App(window)) => return_focus(platform, label, window),
        Release::Window(ReturnTo::Own(own)) => focus_own(&own),
        Release::Nothing => {
            log::debug!(
                "dock window {label:?} gives the focus back; nothing was in front at the press, or a click superseded it"
            );
            Ok(())
        }
        Release::Stale(other) => {
            log::debug!(
                "dock window {label:?} gives the focus back; the session of {other:?} is stale and dropped"
            );
            Ok(())
        }
        Release::Held(other) => {
            log::debug!(
                "dock window {label:?} gives the focus back, but the session is {other:?}'s, which is still in front; it stays"
            );
            Ok(())
        }
        Release::NoSession => {
            log::debug!("dock window {label:?} gives the focus back; nothing to go back to");
            Ok(())
        }
    }
}

/// Bring Stem's own window `own` (the settings window) back to the
/// foreground for the dock window `label` that gives the focus back:
/// Tauri's `set_focus`, which runs on the main thread (`SetForegroundWindow`;
/// the dock in front belongs to that thread, so Windows allows it). A
/// window that closed meanwhile leaves the foreground alone (debug); a
/// failure is returned. Any thread.
fn focus_own_window(app: &AppHandle, label: &str, own: &str) -> Result<(), String> {
    let Some(window) = app.get_webview_window(own) else {
        log::debug!(
            "dock window {label:?} gives the focus back, but {own:?} is gone; the foreground stays"
        );
        return Ok(());
    };
    match window.set_focus() {
        Ok(()) => {
            log::debug!("dock window {label:?} gave the focus back to {own:?}");
            Ok(())
        }
        Err(e) => Err(format!(
            "dock window {label:?} could not give the focus back to {own:?}: {e}"
        )),
    }
}

/// Bring `window` back to the foreground for the dock window `label` that
/// gives the focus back: `Activate` through the platform.
/// A window that is gone leaves the foreground alone (debug); any other
/// failure is returned. Blocking (bounded): never on the main thread.
pub fn return_focus(platform: &dyn Platform, label: &str, window: WindowId) -> Result<(), String> {
    match platform.window_action(window, WindowAction::Activate) {
        Ok(()) => {
            log::debug!("dock window {label:?} gave the focus back to {window:?}");
            Ok(())
        }
        Err(PlatformError::WindowNotFound(_)) => {
            log::debug!(
                "dock window {label:?} gives the focus back, but {window:?} is gone; the foreground stays"
            );
            Ok(())
        }
        Err(e) => Err(format!(
            "dock window {label:?} could not give the focus back to {window:?}: {e}"
        )),
    }
}

/// A click on a dock (`primary_click`, `launch_app`, `window_action`)
/// activates or launches something, which supersedes the window the focus
/// shortcut remembered ([`DockFocus::supersede`]: the window is
/// forgotten, the session stays). Any thread.
pub fn forget_dock_focus(app: &AppHandle, why: &str) {
    let Some(focus) = app.try_state::<DockFocus>() else {
        return;
    };
    if let Some(dock) = focus.supersede() {
        log::debug!("dock window {dock:?}: {why}; the window to go back to is forgotten");
    }
}

/// End the session if it is the dock window `label`'s
/// ([`DockFocus::end`]). Any thread.
pub(super) fn forget_session_of(app: &AppHandle, label: &str, why: &str) {
    let Some(focus) = app.try_state::<DockFocus>() else {
        return;
    };
    if let Some(ended) = focus.end(label) {
        log::debug!("dock window {label:?}: {why}; {}", end_outcome(&ended));
    }
}

/// The dock window `label`'s webview lost the keyboard focus (Tauri's
/// `Focused(false)`; [`DockFocus::blurred`] with the dock window
/// that is in front now). Main thread.
pub(super) fn on_dock_blurred(app: &AppHandle, label: &str) {
    let (Some(service), Some(focus)) = (
        app.try_state::<crate::AppService>(),
        app.try_state::<DockFocus>(),
    ) else {
        return;
    };
    let front = focused_dock(app, service.0.platform().as_ref());
    if let Some(ended) = focus.blurred(label, front.as_deref()) {
        log::debug!(
            "dock window {label:?}: it lost the activation; {}",
            end_outcome(&ended)
        );
    }
}
