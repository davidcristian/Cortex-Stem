//! The set of dock windows: created at startup, made to match the plan
//! after a change, and the windows that are no longer in it closed.

use stem_core::MonitorInfo;
use stem_core::settings::MonitorMode;
use stem_service::DockService;
use tauri::{AppHandle, Manager};

use super::DOCK_LABEL;
use super::create::create_dock_window;
use super::focus_release::forget_session_of;
use super::hit_region::describe_click_through;
use super::plan::{describe_monitor, dock_label, plan_docks};
use super::relayout::{post_to_main_thread, relayout_dock};
use super::state::DockGeometry;
use super::visibility::DockAutoHidden;

/// Create the dock window(s) for the current settings and monitor list, at
/// startup (inside `setup`, on the main thread). With no monitor reported
/// yet only the primary's window exists; the first `dock://monitors`
/// creates the others when the mode asks for them.
pub fn create_dock_windows(app: &AppHandle, service: &DockService) -> tauri::Result<()> {
    let settings = service.settings();
    let click_through = settings.behavior.click_through;
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        *geometry.click_through.lock() = Some(click_through);
        *geometry.mode.lock() = Some(settings.monitors);
        *geometry.position.lock() = Some(settings.position);
    }
    log::info!(
        "click-through outside the panel: {}",
        describe_click_through(click_through)
    );
    let monitors = service.monitors();
    log::info!(
        "dock windows: {:?} on the {:?} edge over {} known monitor(s)",
        settings.monitors,
        settings.position,
        monitors.len()
    );
    reconcile_docks(app, service, settings.monitors, &monitors)
}

/// Make the set of dock windows match [`plan_docks`] for `mode` over
/// `monitors`: close the windows whose monitor is gone (never the
/// primary's), create the missing ones, and lay every remaining one out on
/// its own monitor (which the panel conversion then reports to the engine
/// with the monitor id). A secondary window that cannot be created is
/// logged and skipped; the primary's failure is returned. A window this
/// wants that is still closing (destroyed by an earlier reconcile, its
/// `Destroyed` still to come: a monitor that dropped and came back before
/// the main thread caught up) is neither laid out nor created: its
/// `Destroyed` runs the reconcile again ([`on_dock_destroyed`]). Main
/// thread.
pub(super) fn reconcile_docks(
    app: &AppHandle,
    service: &DockService,
    mode: MonitorMode,
    monitors: &[MonitorInfo],
) -> tauri::Result<()> {
    let plan = plan_docks(mode, monitors);
    if mode == MonitorMode::All {
        for monitor in monitors {
            if !plan.iter().any(|p| p.monitor.as_ref() == Some(monitor)) {
                log::warn!(
                    "monitor {} gets no dock: its label {:?} is already taken",
                    monitor.id.0,
                    dock_label(&monitor.id)
                );
            }
        }
    }

    let existing = app
        .try_state::<DockGeometry>()
        .map(|geometry| geometry.labels())
        .unwrap_or_default();
    for label in existing {
        if label != DOCK_LABEL && !plan.iter().any(|p| p.label == label) {
            close_dock_window(app, service, &label);
        }
    }

    for entry in plan {
        if app
            .try_state::<DockGeometry>()
            .is_some_and(|geometry| geometry.defer_if_closing(&entry.label))
        {
            log::debug!(
                "dock window {:?} is still closing; reconciling again once it is gone",
                entry.label
            );
            continue;
        }
        match app.get_webview_window(&entry.label) {
            Some(window) => {
                let monitor_id = entry.monitor.as_ref().map(|m| m.id.clone());
                let previous = app.try_state::<DockGeometry>().and_then(|geometry| {
                    geometry.update(&entry.label, |dock| {
                        std::mem::replace(&mut dock.monitor, monitor_id.clone())
                    })
                });
                if previous.as_ref() != Some(&monitor_id) {
                    log::info!(
                        "dock window {:?} now on {}",
                        entry.label,
                        describe_monitor(entry.monitor.as_ref())
                    );
                }
                if let Err(e) = relayout_dock(app, &window, monitors) {
                    log::warn!(
                        "re-positioning dock window {:?} after a monitor change failed: {e}",
                        entry.label
                    );
                }
            }
            None => {
                let result = create_dock_window(app, service, &entry.label, entry.monitor.as_ref());
                match result {
                    Ok(()) => {}
                    Err(e) if entry.label == DOCK_LABEL => return Err(e),
                    Err(e) => log::warn!(
                        "dock window {:?} for monitor {} could not be created: {e}",
                        entry.label,
                        entry.monitor.as_ref().map_or("?", |m| m.id.0.as_str())
                    ),
                }
            }
        }
    }
    Ok(())
}

/// Close the secondary dock window `label` (its monitor went away, or the
/// mode went back to the primary only): destroy it (a plain close would be
/// refused like a user's), forget its state, and tell the engine the dock
/// is gone. Tauri only posts the destroy: until the window's `Destroyed`
/// the label stays registered and its page may still report, so the label
/// is marked closing ([`DockGeometry::start_closing`]); with its state
/// gone, every report and every posted relayout for it is dropped, and a
/// reconcile waits for [`on_dock_destroyed`]. Main thread.
fn close_dock_window(app: &AppHandle, service: &DockService, label: &str) {
    let geometry = app.try_state::<DockGeometry>();
    let monitor = geometry
        .as_ref()
        .and_then(|geometry| geometry.docks.lock().remove(label))
        .and_then(|dock| dock.monitor);
    if let Some(flags) = app.try_state::<DockAutoHidden>() {
        flags.0.lock().remove(label);
    }
    forget_session_of(app, label, "it closed");
    crate::appbar::forget(app, label);
    match app.get_webview_window(label) {
        Some(window) => {
            if let Some(geometry) = geometry.as_ref() {
                geometry.start_closing(label);
            }
            if let Err(e) = window.destroy() {
                log::warn!("dock window {label:?} could not be closed: {e}");
                // No `Destroyed` follows a destroy that was never posted.
                if let Some(geometry) = geometry.as_ref() {
                    geometry.finish_closing(label);
                }
            }
        }
        None => log::debug!("dock window {label:?} was already gone"),
    }
    service.set_dock_panel(label, None, None);
    log::info!(
        "dock window closed {label:?} (monitor {})",
        monitor.as_ref().map_or("unknown", |m| m.0.as_str())
    );
}

/// The dock window `label` is gone (`WindowEvent::Destroyed`; Tauri freed
/// the label before it called the window's listeners). For a window
/// [`close_dock_window`] destroyed: forget any bar a late report recreated
/// for it while it was closing (`appbar::forget`), and when a reconcile
/// wanted the label meanwhile, run the reconcile again for the mode and
/// monitors of the moment, at the next turn of the main thread's loop, so
/// a monitor that dropped and came back ends with its dock. Nothing for the
/// windows the quit destroys. Main thread.
pub(super) fn on_dock_destroyed(app: &AppHandle, label: &str) {
    let Some(again) = app
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.finish_closing(label))
    else {
        return;
    };
    crate::appbar::forget(app, label);
    log::debug!("dock window {label:?} is gone");
    if !again {
        return;
    }
    log::info!(
        "dock window {label:?} is gone; reconciling the dock windows again, which wanted it"
    );
    let handle = app.clone();
    let label = label.to_owned();
    post_to_main_thread(app, move || {
        let Some(service) = handle.try_state::<crate::AppService>() else {
            return;
        };
        let mode = service.0.settings().monitors;
        let monitors = service.0.monitors();
        if let Err(e) = reconcile_docks(&handle, &service.0, mode, &monitors) {
            log::warn!("re-creating the dock windows after {label:?} closed failed: {e}");
        }
    });
}
