//! Looking up the dock windows, their native handles and the monitors
//! they were placed on.

use stem_core::{MonitorId, MonitorInfo};
use stem_platform::NativeWindowHandle;
use tauri::{AppHandle, Manager, WebviewWindow};

use super::state::DockGeometry;

/// The OS handle of a Tauri window, for `Platform` calls. `None` where the
/// platform has no native handles (non-Windows hosts) or the lookup fails.
pub fn native_handle(window: &WebviewWindow) -> Option<NativeWindowHandle> {
    #[cfg(windows)]
    {
        match window.hwnd() {
            Ok(hwnd) => Some(NativeWindowHandle(hwnd.0 as isize)),
            Err(e) => {
                log::warn!("no HWND for {:?}: {e}", window.label());
                None
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}

/// The monitor the dock window `label` was placed on, as the state says
/// and the list `monitors` knows it; `None` before the first monitor list,
/// or while the monitor is gone from the list (a hot-plug the reconcile
/// has not caught up with yet).
pub(super) fn monitor_of(
    app: &AppHandle,
    label: &str,
    monitors: &[MonitorInfo],
) -> Option<MonitorInfo> {
    let id = app
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.dock(label))
        .and_then(|dock| dock.monitor)?;
    monitors.iter().find(|m| m.id == id).cloned()
}

/// The monitor id the dock window `label` was placed on, for the engine.
pub(super) fn monitor_id_of(app: &AppHandle, label: &str) -> Option<MonitorId> {
    app.try_state::<DockGeometry>()
        .and_then(|geometry| geometry.dock(label))
        .and_then(|dock| dock.monitor)
}

/// The dock window `label` while it is live: Tauri still has it and so does
/// [`DockGeometry`] (a window [`close_dock_window`] destroyed keeps its
/// label in Tauri until its `Destroyed`, without state here). Work posted
/// for a dock window looks it up through this.
pub(super) fn live_dock_window(app: &AppHandle, label: &str) -> Option<WebviewWindow> {
    let live = app
        .try_state::<DockGeometry>()
        .is_none_or(|geometry| geometry.dock(label).is_some());
    if live {
        app.get_webview_window(label)
    } else {
        None
    }
}

/// The dock windows that exist, the primary's first (never one that is
/// closing: it has no state, [`live_dock_window`]).
pub(crate) fn dock_windows(app: &AppHandle) -> Vec<WebviewWindow> {
    let labels = app
        .try_state::<DockGeometry>()
        .map(|geometry| geometry.labels())
        .unwrap_or_default();
    labels
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .collect()
}
