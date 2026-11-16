//! The hit region of a dock window, and the `behavior.clickThrough`
//! setting that turns it on and off.

use stem_core::Rect;
use stem_core::settings::DockPosition;
use stem_platform::{HitRegion, PlatformError};
use stem_service::DockService;
use tauri::{AppHandle, Manager, WebviewWindow};

use super::lookup::{dock_windows, native_handle};
use super::placement::scale_rect;
use super::state::{DockGeometry, log_closing, log_stale};

pub(super) fn describe_click_through(enabled: bool) -> &'static str {
    if enabled {
        "on"
    } else {
        "off (the whole dock window takes clicks)"
    }
}

/// Argument check for `set_dock_hit_region` (`docs/ipc/hit-region.md`):
/// non-negative sizes, and the envelope contains the panel. Pure.
pub fn validate_hit_region(panel: Rect, envelope: Rect) -> Result<(), String> {
    for (name, rect) in [("panel", panel), ("envelope", envelope)] {
        if rect.width < 0 || rect.height < 0 {
            return Err(format!("{name} has a negative size: {rect:?}"));
        }
    }
    let contains = envelope.x <= panel.x
        && envelope.y <= panel.y
        && envelope.right() >= panel.right()
        && envelope.bottom() >= panel.bottom();
    if !contains {
        return Err(format!(
            "envelope {envelope:?} does not contain panel {panel:?}"
        ));
    }
    Ok(())
}

/// Remember the UI's hit region (CSS px, window-local), measured for
/// `edge`, and, while `behavior.clickThrough` is on, hand it to the
/// platform in physical pixels. While the setting is off the window stays
/// fully clickable and the report waits for the setting to be turned on
/// ([`on_settings_changed`]). A report measured for another edge than the
/// window's is dropped ([`DockWindowState::take_hit_region`]): the
/// platform keeps the region it has until the page's report for the new
/// edge. A platform without the feature (mock, non-Windows hosts) leaves
/// the window fully clickable; that is logged once at debug and is not an
/// error for the UI. Runs on the main thread.
pub fn apply_hit_region(
    app: &AppHandle,
    window: &WebviewWindow,
    panel: Rect,
    envelope: Rect,
    edge: DockPosition,
) -> Result<(), String> {
    let service = app
        .try_state::<crate::AppService>()
        .ok_or_else(|| "service not available".to_owned())?;
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        let taken = geometry.update(window.label(), |dock| {
            dock.take_hit_region(panel, envelope, edge)
        });
        match taken {
            Some(Ok(())) => {}
            Some(Err(placed)) => {
                log_stale(window.label(), "a hit region", edge, placed);
                return Ok(());
            }
            None => {
                log_closing(window.label(), "a hit region");
                return Ok(());
            }
        }
    }
    if !service.0.settings().behavior.click_through {
        log::trace!(
            "hit region of {:?} remembered; click-through is off",
            window.label()
        );
        return Ok(());
    }
    push_hit_region(&service.0, window, panel, envelope)
}

/// The platform call behind [`apply_hit_region`]: CSS px → physical px by
/// the window's scale factor, then `set_dock_hit_region`. Main thread (the
/// scale factor is a window getter).
fn push_hit_region(
    service: &DockService,
    window: &WebviewWindow,
    panel: Rect,
    envelope: Rect,
) -> Result<(), String> {
    let Some(handle) = native_handle(window) else {
        log::debug!("hit region ignored: no native window handle on this host");
        return Ok(());
    };
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let region = HitRegion {
        panel: scale_rect(panel, scale),
        envelope: scale_rect(envelope, scale),
    };
    match service.platform().set_dock_hit_region(handle, Some(region)) {
        Ok(()) => {
            log::trace!("hit region of {:?}: {region:?}", window.label());
            Ok(())
        }
        Err(PlatformError::Unsupported(task)) => {
            static LOGGED: std::sync::Once = std::sync::Once::new();
            LOGGED.call_once(|| {
                log::debug!("hit regions are not supported by this platform (feature: {task})");
            });
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

pub(super) fn on_click_through_setting(app: &AppHandle, enabled: bool) {
    let Some(geometry) = app.try_state::<DockGeometry>() else {
        return;
    };
    {
        let mut current = geometry.click_through.lock();
        if *current == Some(enabled) {
            return;
        }
        *current = Some(enabled);
    }
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply_click_through(&handle, enabled)) {
        log::warn!("could not schedule the click-through change: {e}");
    }
}

/// Turn click-through on (replay each UI's latest hit region) or off (the
/// whole dock windows take clicks again), for every dock window. Main
/// thread.
fn apply_click_through(app: &AppHandle, enabled: bool) {
    let Some(service) = app.try_state::<crate::AppService>() else {
        return;
    };
    let windows = dock_windows(app);
    if windows.is_empty() {
        log::debug!("click-through changed before any dock window exists");
        return;
    }
    if enabled {
        let mut waiting = 0;
        for window in &windows {
            let region = app
                .try_state::<DockGeometry>()
                .and_then(|geometry| geometry.dock(window.label()))
                .and_then(|dock| dock.hit_region_css);
            match region {
                Some((panel, envelope)) => {
                    if let Err(e) = push_hit_region(&service.0, window, panel, envelope) {
                        log::warn!(
                            "click-through could not be turned on for {:?}: {e}",
                            window.label()
                        );
                    }
                }
                None => waiting += 1,
            }
        }
        if waiting > 0 {
            log::info!(
                "click-through outside the panel: on (waiting for the hit region of {waiting} dock window(s))"
            );
        } else {
            log::info!(
                "click-through outside the panel: {}",
                describe_click_through(true)
            );
        }
        return;
    }
    for window in &windows {
        let Some(handle) = native_handle(window) else {
            continue;
        };
        if let Err(e) = service.0.platform().set_dock_hit_region(handle, None)
            && !matches!(e, PlatformError::Unsupported(_))
        {
            log::warn!(
                "click-through could not be turned off for {:?}: {e}",
                window.label()
            );
        }
    }
    log::info!(
        "click-through outside the panel: {}",
        describe_click_through(false)
    );
}

/// Hand a dock window's latest hit region to the platform again, converted
/// at the window's current scale factor (a DPI change), while
/// `behavior.clickThrough` is on. Nothing to do before the UI's first report
/// or while the setting is off. Main thread.
pub(super) fn replay_hit_region(app: &AppHandle, window: &WebviewWindow) -> Result<(), String> {
    let (Some(service), Some(geometry)) = (
        app.try_state::<crate::AppService>(),
        app.try_state::<DockGeometry>(),
    ) else {
        return Ok(());
    };
    if !service.0.settings().behavior.click_through {
        return Ok(());
    }
    let region = geometry
        .dock(window.label())
        .and_then(|dock| dock.hit_region_css);
    match region {
        Some((panel, envelope)) => push_hit_region(&service.0, window, panel, envelope),
        None => Ok(()),
    }
}
