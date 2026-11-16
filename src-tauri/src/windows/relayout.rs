//! Laying a dock window out again from what is remembered for it: after
//! a DPI change, a new band, a monitor change or an edge change.

use stem_core::MonitorInfo;
use stem_core::settings::DockPosition;
use tauri::{AppHandle, Manager, PhysicalSize, WebviewWindow};

use super::hit_region::replay_hit_region;
use super::layout::layout;
use super::lookup::{live_dock_window, monitor_of};
use super::state::DockGeometry;

/// A dock window's scale factor changed (`WindowEvent::ScaleFactorChanged`):
/// the display scale was changed, or the window landed on a monitor
/// with another DPI. Runs on the main thread, inside tao's handling of
/// `WM_DPICHANGED`, which resizes the window to keep its logical size once
/// this returns; the rest is therefore posted for the next turn of the event
/// loop ([`post_to_main_thread`]): the window's physical rect from the
/// remembered content size ([`relayout_dock`], which also converts the
/// remembered panel rect), then the remembered hit region at the new scale
/// ([`replay_hit_region`]). The UI watches the ratio in the browser on its
/// own, re-requests its icons and reports its geometry again; those reports
/// arrive after this and redo the same conversions with the same numbers.
pub(super) fn on_scale_factor_changed(
    app: &AppHandle,
    label: &str,
    scale: f64,
    new_inner_size: PhysicalSize<u32>,
) {
    let previous = app
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.update(label, |dock| dock.scale.replace(scale)))
        .flatten();
    log::info!(
        "dock window {label:?}: scale factor {} -> {scale} (inner size now {}x{} px); laying it out again",
        previous.map_or_else(|| "?".to_owned(), |s| s.to_string()),
        new_inner_size.width,
        new_inner_size.height
    );
    let handle = app.clone();
    let label = label.to_owned();
    post_to_main_thread(app, move || {
        let Some(window) = live_dock_window(&handle, &label) else {
            log::debug!("dock window {label:?} is gone before its scale change was applied");
            return;
        };
        let monitors = handle
            .try_state::<crate::AppService>()
            .map_or_else(Vec::new, |service| service.0.monitors());
        if let Err(e) = relayout_dock(&handle, &window, &monitors) {
            log::warn!("laying out dock window {label:?} again after a scale change failed: {e}");
        }
        if let Err(e) = replay_hit_region(&handle, &window) {
            log::warn!(
                "the hit region of {label:?} could not be converted after a scale change: {e}"
            );
        }
    });
}

/// Run `f` on the main thread at the next turn of the event loop.
/// `AppHandle::run_on_main_thread` runs its task at once when it is already
/// on the main thread (tauri-runtime-wry's `send_user_message`), which is
/// too early from inside a window event callback: tao is still handling the
/// message that raised the event and sizes the window itself afterwards. A
/// hop through the async runtime turns the task into a posted message.
pub(super) fn post_to_main_thread(app: &AppHandle, f: impl FnOnce() + Send + 'static) {
    let handle = app.clone();
    drop(tauri::async_runtime::spawn(async move {
        if let Err(e) = handle.run_on_main_thread(f) {
            log::warn!("could not schedule work on the main thread: {e}");
        }
    }));
}

/// The shell granted the dock window `label` a new band
/// (`appbar`): lay the window out again at the next turn of the main
/// thread's loop, which puts it on the band ([`layout`]). Any thread.
pub(crate) fn relayout_later(app: &AppHandle, label: &str) {
    let handle = app.clone();
    let label = label.to_owned();
    post_to_main_thread(app, move || {
        let Some(window) = live_dock_window(&handle, &label) else {
            log::debug!("dock window {label:?} is gone before it could go onto its band");
            return;
        };
        let monitors = handle
            .try_state::<crate::AppService>()
            .map_or_else(Vec::new, |service| service.0.monitors());
        if let Err(e) = relayout_dock(&handle, &window, &monitors) {
            log::warn!("dock window {label:?} could not be placed on its band: {e}");
        }
    });
}

/// What decides the band the dock window asks for
/// (`appbar::request_for`): whether it is shown, the bounds of the monitor
/// and the edge it was placed on, the edge in effect, the panel's distance
/// to its edge side from the page's latest report, and its scale. Main
/// thread (the getters are window calls).
pub(crate) fn app_bar_inputs(
    app: &AppHandle,
    window: &WebviewWindow,
    enabled: bool,
) -> crate::appbar::BarInputs {
    let dock = app
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.dock(window.label()))
        .unwrap_or_default();
    let monitor = dock.monitor.as_ref().and_then(|id| {
        app.try_state::<crate::AppService>().and_then(|service| {
            service
                .0
                .monitors()
                .into_iter()
                .find(|m| &m.id == id)
                .map(|m| m.bounds)
        })
    });
    crate::appbar::BarInputs {
        enabled,
        visible: window.is_visible().unwrap_or(false),
        monitor,
        placed: dock.edge,
        edge: dock_edge(app),
        reserve_css: dock.reserve_css,
        scale: window.scale_factor().unwrap_or(1.0),
    }
}

/// Size and place a dock window on the edge in effect ([`dock_edge`]) of its
/// own monitor's work area from what is remembered for it: [`layout`] with
/// its UI's latest content size, or none before the first report. The one
/// path behind a monitor or work-area change, a DPI change
/// and an edge change. Main thread.
pub(super) fn relayout_dock(
    app: &AppHandle,
    window: &WebviewWindow,
    monitors: &[MonitorInfo],
) -> tauri::Result<()> {
    let content = app
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.dock(window.label()))
        .and_then(|dock| dock.content);
    let monitor = monitor_of(app, window.label(), monitors);
    layout(window, monitor.as_ref(), dock_edge(app), content)
}

/// The screen edge the dock windows are laid out on: `settings.position` as
/// this module last took it ([`create_dock_windows`], then
/// [`on_position_setting`]), else the service's, else the bottom.
pub(super) fn dock_edge(app: &AppHandle) -> DockPosition {
    app.try_state::<DockGeometry>()
        .and_then(|geometry| *geometry.position.lock())
        .or_else(|| {
            app.try_state::<crate::AppService>()
                .map(|service| service.0.settings().position)
        })
        .unwrap_or(DockPosition::Bottom)
}
