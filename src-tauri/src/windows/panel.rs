//! The resting panel a dock window's page reports, converted to screen
//! pixels for the engine, the backdrop and the band.

use stem_core::Rect;
use stem_service::DockService;
use tauri::{AppHandle, Manager, WebviewWindow};

use super::lookup::monitor_id_of;
use super::placement::scale_rect;
use super::state::{DockGeometry, log_closing, log_stale};
use super::viewport::{CssViewport, PanelReport};

/// Argument check for `set_dock_panel_rect` (`docs/ipc/dock-panel.md`):
/// non-negative sizes. Pure.
pub fn validate_panel_rect(rect: Rect) -> Result<(), String> {
    if rect.width < 0 || rect.height < 0 {
        return Err(format!("the panel has a negative size: {rect:?}"));
    }
    Ok(())
}

/// Remember the UI's resting panel (CSS px of the viewport it carries,
/// measured for the edge it names) and hand it to the service in physical
/// screen pixels, with the window's label and monitor,
/// re-anchored to the window's viewport of the moment
/// ([`push_dock_panel`]); no rect goes instead while the window is hidden. A
/// report measured for another edge than the window's is dropped
/// ([`DockWindowState::take_panel`]). The panel's distance to its
/// viewport's edge side is the band *Reserve screen space* asks for
/// (`appbar::sync`), read against the report's own viewport, so a report
/// handled after the resize the page sent its content size for (the same
/// tick, applied first) says what it said when the page measured it. Runs
/// on the main thread.
///
/// [`DockWindowState::take_panel`]: super::state::DockWindowState::take_panel
pub fn apply_dock_panel(
    app: &AppHandle,
    window: &WebviewWindow,
    report: PanelReport,
) -> Result<(), String> {
    let service = app
        .try_state::<crate::AppService>()
        .ok_or_else(|| "service not available".to_owned())?;
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        let taken = geometry.update(window.label(), |dock| {
            (dock.take_panel(report), dock.reserve_css)
        });
        match taken {
            Some((Err(placed), _)) => {
                log_stale(window.label(), "a panel rect", report.edge, placed);
                return Ok(());
            }
            Some((Ok(None), reserve)) => log::debug!(
                "dock window {:?}: the panel {:?} does not lie inside the {:?} css viewport it was measured in across the {:?} edge; the band keeps {reserve:?} css",
                window.label(),
                report.rect,
                report.viewport,
                report.edge
            ),
            Some((Ok(Some(_)), _)) => {}
            None => {
                log_closing(window.label(), "a panel rect");
                return Ok(());
            }
        }
    }
    let pushed = push_dock_panel(app, &service.0, window, report, None);
    crate::appbar::sync(app, window);
    pushed
}

/// The window's viewport in CSS px now (its client size over its scale
/// factor, not rounded), the space the page lays itself out in; `None`
/// (logged at debug) when the window cannot say.
fn viewport_now(window: &WebviewWindow) -> Option<CssViewport> {
    let read = || -> tauri::Result<CssViewport> {
        let size = window.inner_size()?;
        let scale = window.scale_factor()?;
        Ok(CssViewport::of((size.width, size.height), scale))
    };
    match read() {
        Ok(viewport) => Some(viewport),
        Err(e) => {
            log::debug!(
                "dock window {:?}: its viewport is unknown ({e}); its panel rect is read as reported",
                window.label()
            );
            None
        }
    }
}

/// Convert the page's panel `report` for the engine and the backdrop: on a
/// hidden window no rect; else the rect re-anchored to the window's
/// viewport now ([`PanelReport::now`]), CSS px → physical screen px
/// from the window's top-left corner (`origin` when the caller just placed
/// it there, else read from the window) and its scale factor
/// ([`panel_on_screen`]), then `DockService::set_dock_panel` for the
/// window's label and monitor and `backdrop::on_panel`. Main thread (the
/// getters are window calls).
fn push_dock_panel(
    app: &AppHandle,
    service: &DockService,
    window: &WebviewWindow,
    report: PanelReport,
    origin: Option<(i32, i32)>,
) -> Result<(), String> {
    let label = window.label();
    let monitor = monitor_id_of(app, label);
    if !window.is_visible().map_err(|e| e.to_string())? {
        service.set_dock_panel(label, monitor, None);
        crate::backdrop::on_panel(app, window, None);
        return Ok(());
    }
    let origin = match origin {
        Some(origin) => origin,
        None => {
            let position = window.outer_position().map_err(|e| e.to_string())?;
            (position.x, position.y)
        }
    };
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let viewport = viewport_now(window);
    let css = report.now(viewport);
    let panel = panel_on_screen(origin, css, scale);
    if css == report.rect {
        log::trace!(
            "dock panel of {label:?}: {css:?} css at {scale}x from {origin:?} -> {panel:?} on {monitor:?}"
        );
    } else {
        log::trace!(
            "dock panel of {label:?}: {:?} css measured in {:?} on the {:?} edge, {css:?} in {viewport:?}, at {scale}x from {origin:?} -> {panel:?} on {monitor:?}",
            report.rect,
            report.viewport,
            report.edge
        );
    }
    service.set_dock_panel(label, monitor, Some(panel));
    crate::backdrop::on_panel(
        app,
        window,
        Some(crate::backdrop::PanelConversion {
            origin,
            css,
            scale,
            viewport,
        }),
    );
    Ok(())
}

/// The window was moved or resized by `layout::place`, or shown
/// again: convert the UI's latest panel report for the window's new place
/// and viewport ([`push_dock_panel`]). `origin` is the window's top-left
/// corner in physical pixels when known (just placed), else it is read from
/// the window. Main thread; silent when the UI has reported nothing yet.
pub(super) fn sync_dock_panel(window: &WebviewWindow, origin: Option<(i32, i32)>) {
    let app = window.app_handle();
    let (Some(service), Some(geometry)) = (
        app.try_state::<crate::AppService>(),
        app.try_state::<DockGeometry>(),
    ) else {
        return;
    };
    let Some(report) = geometry.dock(window.label()).and_then(|dock| dock.panel) else {
        return;
    };
    if let Err(e) = push_dock_panel(app, &service.0, window, report, origin) {
        log::debug!(
            "the dock panel of {:?} could not be converted after a move: {e}",
            window.label()
        );
    }
}

/// The resting panel in physical pixels of the virtual desktop: `css`
/// scaled like a hit region (`scale_rect`, rounded outwards) and offset by
/// the window's top-left corner `origin` (physical px). Pure.
pub fn panel_on_screen(origin: (i32, i32), css: Rect, scale: f64) -> Rect {
    let scaled = scale_rect(css, scale);
    Rect::new(
        origin.0.saturating_add(scaled.x),
        origin.1.saturating_add(scaled.y),
        scaled.width,
        scaled.height,
    )
}
