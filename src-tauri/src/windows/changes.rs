//! A settings change or a new monitor list, applied to the dock windows.

use stem_core::settings::{DockPosition, MonitorMode};
use stem_core::{MonitorInfo, Settings};
use stem_service::DockService;
use tauri::{AppHandle, Manager};

use super::DOCK_LABEL;
use super::hit_region::on_click_through_setting;
use super::lookup::dock_windows;
use super::reconcile::reconcile_docks;
use super::relayout::relayout_dock;
use super::settings_window::on_theme_setting;
use super::state::DockGeometry;

/// The settings changed (settings window, tray, a command): apply
/// `behavior.clickThrough`, `theme`, `position`
/// and `monitors` where they differ from what is in effect. Runs on
/// the service worker thread; the window work hops to the main thread,
/// where the window getters are direct calls. The edge is taken before the
/// monitor mode, so windows a mode change creates already go to the new
/// edge.
pub fn on_settings_changed(app: &AppHandle, settings: &Settings) {
    on_click_through_setting(app, settings.behavior.click_through);
    on_theme_setting(app, settings.theme);
    on_position_setting(app, settings.position);
    on_monitor_mode_setting(app, settings.monitors);
}

/// `settings.position` changed (with `monitors`, one of the two
/// changes `Settings::diff_requires_window_rebuild` names): move every dock
/// window to the new edge of its own monitor's work area
/// (`docs/ipc/sizing-protocol.md`, step 5). The windows are moved in place,
/// not created anew: no window attribute depends on the edge (only its rect
/// does), a new window would reload its page for nothing (a blank frame,
/// every icon fetched again, the hover and auto-hide state lost), and a
/// monitor change already re-places windows the same way
/// ([`relayout_dock`]). Each window keeps its thickness, now across the new
/// edge, until its page reports a content size for that edge
/// ([`DockContent::on_edge`]); what its page laid out for the old edge is
/// dropped first ([`forget_edge_layout`]), and the primary's backdrop and
/// every band go with it (`backdrop::on_panel`, `appbar::sync`) until the
/// page reports its panel on the new edge. The pages get `dock://settings`
/// before this runs (`lib.rs`, `forward_events`), so a page usually lays
/// itself out for the new edge before its window moves. Any thread; the
/// window work hops to the main thread.
fn on_position_setting(app: &AppHandle, edge: DockPosition) {
    let Some(geometry) = app.try_state::<DockGeometry>() else {
        return;
    };
    let previous = {
        let mut current = geometry.position.lock();
        if *current == Some(edge) {
            return;
        }
        current.replace(edge)
    };
    let handle = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        let Some(service) = handle.try_state::<crate::AppService>() else {
            return;
        };
        let windows = dock_windows(&handle);
        log::info!(
            "dock windows: moving {} window(s) from the {} edge to the {edge:?} edge",
            windows.len(),
            previous.map_or_else(|| "unknown".to_owned(), |p| format!("{p:?}"))
        );
        let monitors = service.0.monitors();
        for window in windows {
            forget_edge_layout(&handle, &service.0, window.label());
            // The backdrop and the band on the old edge go now,
            // before the window moves: no material without its panel, and
            // the move converts no panel rect (there is none until the page
            // reports one), so nothing else would take the material away.
            // Both come back with the page's first panel report there.
            crate::backdrop::on_panel(&handle, &window, None);
            crate::appbar::sync(&handle, &window);
            if let Err(e) = relayout_dock(&handle, &window, &monitors) {
                log::warn!(
                    "dock window {:?} could not be moved to the {edge:?} edge: {e}",
                    window.label()
                );
            }
        }
    });
    if let Err(e) = scheduled {
        log::warn!("could not schedule the move to the {edge:?} edge: {e}");
    }
}

/// Before the dock window `label` moves to another edge: forget the
/// panel rect and the hit region its page laid out for the old edge, and
/// tell the engine the dock has no panel until the page reports one for
/// the new edge (as while the window is hidden). Converted again at the
/// window's new origin, the old rect would lie where no panel is (a bottom
/// panel 700 px long running from a left window 135 px wide into the
/// desktop), and the engine would compute the dock's `overlapped` from it:
/// with *When a window overlaps* the dock could slide out and back
/// for nothing. The platform keeps the hit region it has (window-local,
/// where the page last drew its panel) until the page's next report; it is
/// not replayed meanwhile (a DPI change, `behavior.clickThrough` turned
/// on). The content size is kept: its thickness holds until the next
/// report (`docs/ipc/sizing-protocol.md`, step 5). A report the page
/// measured for the old edge that is handled after this is dropped
/// ([`DockWindowState::stale_for`]): the window is placed on the new edge
/// in the same turn of the main thread. Main thread.
fn forget_edge_layout(app: &AppHandle, service: &DockService, label: &str) {
    let Some(geometry) = app.try_state::<DockGeometry>() else {
        return;
    };
    let forgotten = geometry.update(label, |dock| {
        dock.hit_region_css = None;
        dock.reserve_css = None;
        dock.panel.take().map(|_| dock.monitor.clone())
    });
    if let Some(Some(monitor)) = forgotten {
        log::debug!(
            "dock window {label:?}: panel rect withdrawn until its page reports for the new edge"
        );
        service.set_dock_panel(label, monitor, None);
    }
}

/// `settings.monitors` changed: create the secondary dock windows
/// (`All`) or close them (`Primary`); the primary's window is never touched.
/// Any thread; the window work hops to the main thread.
fn on_monitor_mode_setting(app: &AppHandle, mode: MonitorMode) {
    let Some(geometry) = app.try_state::<DockGeometry>() else {
        return;
    };
    {
        let mut current = geometry.mode.lock();
        if *current == Some(mode) {
            return;
        }
        *current = Some(mode);
    }
    let handle = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        let Some(service) = handle.try_state::<crate::AppService>() else {
            return;
        };
        let monitors = service.0.monitors();
        log::info!(
            "dock windows: {mode:?} over {} known monitor(s)",
            monitors.len()
        );
        if let Err(e) = reconcile_docks(&handle, &service.0, mode, &monitors) {
            log::warn!("the dock windows could not be rebuilt for {mode:?}: {e}");
        }
    });
    if let Err(e) = scheduled {
        log::warn!("could not schedule the dock window rebuild: {e}");
    }
}

/// The service reported a new monitor list (startup, hot-plug, resolution
/// or taskbar change, a new primary): reconcile the dock windows with it
/// and lay each out on its monitor. Runs on the service worker
/// thread; the window work hops to the main thread so the blocking getters
/// run there directly.
pub fn on_monitors_changed(app: &AppHandle, monitors: Vec<MonitorInfo>) {
    let handle = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        let Some(service) = handle.try_state::<crate::AppService>() else {
            return;
        };
        if handle.get_webview_window(DOCK_LABEL).is_none() {
            log::debug!("monitors changed before the dock window exists");
            return;
        }
        let mode = service.0.settings().monitors;
        if let Err(e) = reconcile_docks(&handle, &service.0, mode, &monitors) {
            log::warn!("re-positioning the docks after a monitor change failed: {e}");
        }
    });
    if let Err(e) = scheduled {
        log::warn!("could not schedule the dock re-layout: {e}");
    }
}
