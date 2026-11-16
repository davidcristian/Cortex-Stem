//! The dock window the focus shortcut gives the keyboard to, the window
//! it gives it back to, and which of Stem's own windows is in front.

use stem_core::settings::MonitorMode;
use stem_core::{DockState, MonitorId, MonitorInfo, Rect, WindowId};
use stem_platform::Platform;
use tauri::{AppHandle, Manager};

use super::DOCK_LABEL;
use super::focus_session::ReturnTo;
use super::lookup::{dock_windows, native_handle};

/// The monitor of the window `id` as the engine's snapshot lists it (its
/// `WindowInfo.monitor`, in whichever item shows it); `None` when no item
/// shows the window or its monitor is unknown. Pure.
pub fn window_monitor(state: &DockState, id: WindowId) -> Option<MonitorId> {
    state
        .items
        .iter()
        .flat_map(|item| item.windows.iter())
        .find(|w| w.id == id)
        .and_then(|w| w.monitor.clone())
}

/// The dock window the focus shortcut gives the focus to: with
/// `monitors = all`, the one placed on `monitor`, the monitor of the window
/// in front; otherwise, or when no dock window sits on that monitor (none
/// known for the window, a monitor that got no dock), the primary's
/// ([`DOCK_LABEL`]). `docks` are the dock windows that exist, each with the
/// monitor it was placed on. Pure.
pub fn focus_target(
    mode: MonitorMode,
    monitor: Option<&MonitorId>,
    docks: &[(String, Option<MonitorId>)],
) -> String {
    if mode == MonitorMode::All
        && let Some(monitor) = monitor
        && let Some((label, _)) = docks.iter().find(|(_, m)| m.as_ref() == Some(monitor))
    {
        return label.clone();
    }
    DOCK_LABEL.to_owned()
}

/// What a press of the focus shortcut gives the keyboard focus back to:
/// `own` is the label of Stem's own window that is the foreground
/// window at the press, if one is ([`own_foreground`]), and `engine` the
/// engine's active window (`DockState.foreground`). The engine never takes
/// Stem's own windows for the foreground, so while the settings window is
/// in front its active window is the app behind it: the settings window is
/// then the one to go back to. A dock window in front (after an Escape that
/// had nothing to go back to) is not one: the engine's window stays the
/// one. Pure.
pub fn return_target(own: Option<&str>, engine: Option<WindowId>) -> Option<ReturnTo> {
    match own {
        Some(label) if !label.starts_with(DOCK_LABEL) => Some(ReturnTo::Own(label.to_owned())),
        _ => engine.map(ReturnTo::App),
    }
}

/// The monitor whose bounds are `bounds` in `monitors` (the platform's
/// list); `None` when none has them. Tauri's `current_monitor` gives a
/// window's monitor by the same rectangle the platform reports as
/// `MonitorInfo.bounds` (the monitor's full rect, physical px). Pure.
pub fn monitor_with_bounds(monitors: &[MonitorInfo], bounds: Rect) -> Option<MonitorId> {
    monitors
        .iter()
        .find(|monitor| monitor.bounds == bounds)
        .map(|monitor| monitor.id.clone())
}

/// The monitor Stem's own window `label` is on (the one with the largest
/// part of it, Tauri's `current_monitor`) as `monitors` lists it; `None`
/// when the window is gone or its monitor is not in the list.
pub(super) fn own_window_monitor(
    app: &AppHandle,
    label: &str,
    monitors: &[MonitorInfo],
) -> Option<MonitorId> {
    let monitor = app.get_webview_window(label)?.current_monitor().ok()??;
    let position = monitor.position();
    let size = monitor.size();
    let bounds = Rect::new(
        position.x,
        position.y,
        i32::try_from(size.width).unwrap_or(i32::MAX),
        i32::try_from(size.height).unwrap_or(i32::MAX),
    );
    monitor_with_bounds(monitors, bounds)
}

/// The label of Stem's own window (a dock window, the settings window) that
/// is the foreground window right now, if one is (the platform's view;
/// never on a host without native handles).
pub(super) fn own_foreground(app: &AppHandle, platform: &dyn Platform) -> Option<String> {
    let foreground = platform.foreground_native_window()?;
    app.webview_windows()
        .into_iter()
        .find(|(_, window)| native_handle(window) == Some(foreground))
        .map(|(label, _)| label)
}

/// Whether one of Stem's own windows other than a dock (the settings
/// window) is the foreground window right now ([`own_foreground`]): the
/// engine's active window is then the app behind it, and a click that would
/// minimise that app brings it forward instead
/// (`DockService::primary_click_over`). A dock in front (the focus
/// shortcut's session) does not count. Any thread: the window getters hop
/// to the main thread.
pub fn non_dock_window_in_front(app: &AppHandle, platform: &dyn Platform) -> bool {
    own_foreground(app, platform).is_some_and(|label| !label.starts_with(DOCK_LABEL))
}

/// The dock window that is the foreground window right now, if one is (the
/// platform's view; never on a host without native handles).
pub(super) fn focused_dock(app: &AppHandle, platform: &dyn Platform) -> Option<String> {
    let foreground = platform.foreground_native_window()?;
    dock_windows(app)
        .into_iter()
        .find(|window| native_handle(window) == Some(foreground))
        .map(|window| window.label().to_owned())
}
