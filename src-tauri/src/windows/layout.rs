//! Sizing and placing a dock window: the content size its page reports,
//! the layout on the edge in effect, and the window calls of a placement.

use stem_core::settings::DockPosition;
use stem_core::{MonitorInfo, Rect};
use stem_platform::rules::appbar::{across, window_rect};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

use super::lookup::monitor_of;
use super::panel::sync_dock_panel;
use super::placement::{
    DockContent, PlaceStep, clamp_content, placement_steps, relayout_rect, thickness_of,
};
use super::relayout::dock_edge;
use super::state::{DockGeometry, log_closing, log_stale};

/// Take the reported content size (CSS px), measured for `edge`: the value
/// across that edge becomes the window's thickness, and the window is laid
/// on the edge in effect of its own monitor's work area, as long as the
/// work area along it ([`layout`]). A report measured for another edge than
/// the window's is dropped ([`DockWindowState::take_content`]),
/// unless it names the edge in effect: then the window's move to that edge
/// failed or is still to come, and this layout is the move (logged). The
/// page's first report for a new edge always includes a content size (it
/// sends one after every position change), before its hit region and panel
/// rect, so those are taken once this layout placed the window. Called from
/// the `set_dock_content_size` command, on the main thread.
pub fn apply_content_size(
    app: &AppHandle,
    window: &WebviewWindow,
    width: f64,
    height: f64,
    edge: DockPosition,
) -> tauri::Result<()> {
    let (width, height) = clamp_content(edge, width, height);
    let content = DockContent {
        width,
        height,
        edge,
    };
    let in_effect = dock_edge(app);
    let taken = app.try_state::<DockGeometry>().and_then(|geometry| {
        geometry.update(window.label(), |dock| dock.take_content(content, in_effect))
    });
    match taken {
        Some(Ok(None)) => {}
        Some(Ok(Some(placed))) => log::info!(
            "dock window {:?}: a content size for the {edge:?} edge while the window is still on the {placed:?} edge; moving it there",
            window.label()
        ),
        Some(Err(placed)) => {
            log_stale(window.label(), "a content size", edge, placed);
            return Ok(());
        }
        None => {
            log_closing(window.label(), "a content size");
            return Ok(());
        }
    }
    let monitors = match app.try_state::<crate::AppService>() {
        Some(service) => service.0.monitors(),
        None => Vec::new(),
    };
    let monitor = monitor_of(app, window.label(), &monitors);
    layout(window, monitor.as_ref(), in_effect, Some(content))
}

/// Size and place the window on `edge` of `monitor`'s work area (Tauri's
/// own view of the window's monitor while none is known): the rect
/// [`relayout_rect`] gives from `content`, the UI's latest content size, or
/// before its first report (`None`) from the window's current size and the
/// edge it was placed on last. While the shell has reserved a band for the
/// window on that edge of that monitor (`appbar::granted_band`),
/// the window keeps that rect's thickness but goes with its edge side on
/// the band's edge side, as long as the band
/// (`stem_platform::rules::appbar::window_rect`). Every path that sizes or
/// moves a dock window comes here (creation, the content size,
/// [`relayout_dock`]); [`place`] applies the rect.
pub(super) fn layout(
    window: &WebviewWindow,
    monitor: Option<&MonitorInfo>,
    edge: DockPosition,
    content: Option<DockContent>,
) -> tauri::Result<()> {
    let work_area = match monitor {
        Some(m) => m.work_area,
        None => fallback_work_area(window)?,
    };
    let scale = window_scale(window, monitor);
    let size: PhysicalSize<u32> = window.outer_size()?;
    let size = (size.width, size.height);
    let placed = window
        .app_handle()
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.dock(window.label()))
        .and_then(|dock| dock.edge);
    let rect = relayout_rect(
        work_area,
        monitor.is_some(),
        edge,
        content,
        scale,
        size,
        placed,
    );
    let band = monitor
        .and_then(|m| crate::appbar::granted_band(window.app_handle(), window.label(), edge, m));
    let rect = match band {
        Some(band) => {
            let on_band = window_rect(band, edge, across(rect, edge));
            log::debug!(
                "dock window {:?}: {rect:?} moved onto the band {band:?} the shell reserved -> {on_band:?}",
                window.label()
            );
            on_band
        }
        None => rect,
    };
    match content {
        Some(content) => {
            let (width, height) = content.on_edge(edge);
            log::debug!(
                "dock window {:?}: {width}x{height} css at {scale}x -> {rect:?} in {work_area:?} (the {edge:?} edge, full length)",
                window.label()
            );
        }
        None => log::debug!(
            "dock window {:?}: no content size yet, {} px across the edge kept at {scale}x -> {rect:?} in {work_area:?} (the {edge:?} edge, full length)",
            window.label(),
            thickness_of(size, placed)
        ),
    }
    place(window, edge, rect)
}

/// The window's scale factor, or its monitor's (1 without one) when the
/// window cannot give a usable one (logged).
fn window_scale(window: &WebviewWindow, monitor: Option<&MonitorInfo>) -> f64 {
    match window.scale_factor() {
        Ok(s) if s.is_finite() && s > 0.0 => s,
        Ok(s) => {
            log::warn!("window scale factor {s} is unusable; using the monitor's");
            monitor.map_or(1.0, |m| m.scale_factor)
        }
        Err(e) => {
            log::warn!("window scale factor unavailable ({e}); using the monitor's");
            monitor.map_or(1.0, |m| m.scale_factor)
        }
    }
}

/// Size and position a dock window at `rect` on `edge`, through the window
/// calls of [`placement_steps`] (never through a rect outside both its old
/// and its new place, so a move between edges does not pass over a
/// neighbouring monitor and change the window's DPI on the way); remember
/// the edge for its next placement as soon as the window has its new size
/// (the size alone says which side is the thickness, [`thickness_of`], so
/// a move that fails after the resize still leaves the right pair); then
/// redo the panel conversion for its new place and its new
/// viewport (the page's latest rect re-anchored to it, so a
/// thickness change or a drag's growth needs no new report from the page)
/// and let `appbar` follow (a new monitor or scale changes the
/// band). Every
/// placement of a dock window ends here.
fn place(window: &WebviewWindow, edge: DockPosition, rect: Rect) -> tauri::Result<()> {
    let current = current_rect(window);
    let target = (rect.width, rect.height);
    let mut size = current.map(|r| (r.width, r.height));
    let mut remembered = false;
    for step in placement_steps(current, rect) {
        if !remembered && size == Some(target) {
            remember_edge(window, edge);
            remembered = true;
        }
        match step {
            PlaceStep::Size { width, height } => {
                window.set_size(PhysicalSize::new(
                    width.unsigned_abs(),
                    height.unsigned_abs(),
                ))?;
                size = Some((width, height));
            }
            PlaceStep::Move { x, y } => window.set_position(PhysicalPosition::new(x, y))?,
        }
    }
    if !remembered {
        remember_edge(window, edge);
    }
    sync_dock_panel(window, Some((rect.x, rect.y)));
    crate::appbar::sync(window.app_handle(), window);
    Ok(())
}

/// Remember `edge` as the one the dock window was placed on ([`place`]).
fn remember_edge(window: &WebviewWindow, edge: DockPosition) {
    if let Some(geometry) = window.app_handle().try_state::<DockGeometry>() {
        geometry.update(window.label(), |dock| dock.edge = Some(edge));
    }
}

/// The window's rect on screen now, in physical pixels; `None` (logged at
/// debug) when the window cannot say, and [`placement_steps`] then resizes
/// before it moves.
fn current_rect(window: &WebviewWindow) -> Option<Rect> {
    let read = || -> tauri::Result<Rect> {
        let position = window.outer_position()?;
        let size = window.outer_size()?;
        Ok(Rect::new(
            position.x,
            position.y,
            i32::try_from(size.width).unwrap_or(i32::MAX),
            i32::try_from(size.height).unwrap_or(i32::MAX),
        ))
    };
    match read() {
        Ok(rect) => Some(rect),
        Err(e) => {
            log::debug!(
                "dock window {:?}: its rect could not be read before a placement ({e})",
                window.label()
            );
            None
        }
    }
}

/// Before the platform reported monitors, fall back to Tauri's own view: the
/// window's monitor, else the primary one (only queried when needed, so its
/// failure cannot mask a successful first answer).
fn fallback_work_area(window: &WebviewWindow) -> tauri::Result<Rect> {
    let monitor = match window.current_monitor()? {
        Some(current) => Some(current),
        None => window.primary_monitor()?,
    };
    Ok(match monitor {
        Some(m) => {
            let pos = m.work_area().position;
            let size = m.work_area().size;
            Rect::new(pos.x, pos.y, size.width as i32, size.height as i32)
        }
        None => Rect::new(0, 0, 1920, 1040),
    })
}
