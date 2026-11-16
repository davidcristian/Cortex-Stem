//! The rects of a band and of the dock window on it: physical pixels of the
//! virtual desktop, half-open like [`Rect`].

use stem_core::Rect;
use stem_core::settings::DockPosition;

use crate::AppBarRequest;

/// Whether a dock on `edge` is vertical (`left`, `right`).
fn is_vertical(edge: DockPosition) -> bool {
    matches!(edge, DockPosition::Left | DockPosition::Right)
}

/// The size of `rect` across `edge`: its height on `bottom` and `top`, its
/// width on `left` and `right`.
pub fn across(rect: Rect, edge: DockPosition) -> i32 {
    if is_vertical(edge) {
        rect.width
    } else {
        rect.height
    }
}

/// The thickness a band for `request` gets: the request's, at least 1 px
/// and at most half the monitor across the edge, so no request (a panel
/// rect read for the wrong edge, say) can take the whole screen.
pub fn band_thickness(request: &AppBarRequest) -> i32 {
    let max = (across(request.monitor, request.edge) / 2).max(1);
    request.thickness.clamp(1, max)
}

/// `rect` cut to `thickness` across `edge`, keeping its side on `edge`
/// where it is (its bottom on `bottom`, its top on `top`, its left side on
/// `left`, its right side on `right`) and its length along the edge.
fn on_edge_side(rect: Rect, edge: DockPosition, thickness: i32) -> Rect {
    let thickness = thickness.max(1);
    let length_x = rect.width.max(1);
    let length_y = rect.height.max(1);
    match edge {
        DockPosition::Bottom => Rect::new(rect.x, rect.bottom() - thickness, length_x, thickness),
        DockPosition::Top => Rect::new(rect.x, rect.y, length_x, thickness),
        DockPosition::Left => Rect::new(rect.x, rect.y, thickness, length_y),
        DockPosition::Right => Rect::new(rect.right() - thickness, rect.y, thickness, length_y),
    }
}

/// The band to propose with `ABM_QUERYPOS` for `request`: on its edge of
/// the monitor's bounds, as long as the bounds along it and
/// [`band_thickness`] across.
pub fn query_rect(request: &AppBarRequest) -> Rect {
    on_edge_side(request.monitor, request.edge, band_thickness(request))
}

/// The band to pass to `ABM_SETPOS` once the shell adjusted the proposal to
/// `adjusted` (`ABM_QUERYPOS` moves its edge side inwards past the bars
/// already on that edge and may trim its ends): the edge side where the
/// shell put it and [`band_thickness`] across again.
pub fn set_rect(adjusted: Rect, request: &AppBarRequest) -> Rect {
    on_edge_side(adjusted, request.edge, band_thickness(request))
}

/// Whether `granted`, the shell's answer to `ABM_SETPOS`, is a band a dock
/// window can be placed on: not empty, and inside the requested monitor.
/// Anything else (a shell that did not answer, a rect on another monitor)
/// is not used.
pub fn is_usable(granted: Rect, request: &AppBarRequest) -> bool {
    let monitor = request.monitor;
    granted.width > 0
        && granted.height > 0
        && granted.x >= monitor.x
        && granted.y >= monitor.y
        && granted.right() <= monitor.right()
        && granted.bottom() <= monitor.bottom()
}

/// The dock window's rect on the band `granted` on `edge`: its edge side on
/// the band's edge side, as long as the band along the edge, and
/// `window_thickness` across (the window's own thickness, which reaches
/// past the band into the work area).
pub fn window_rect(granted: Rect, edge: DockPosition, window_thickness: i32) -> Rect {
    on_edge_side(granted, edge, window_thickness)
}

/// Whether `work_area` still leaves out the band `band` on `edge`: its side
/// on that edge stops at the band's far side or further in. True from the
/// moment the shell reserved the band until the moment it gave it back and
/// the work area grew over it again, so a band that was just released can
/// still be placed on while the work area (as last reported) has not caught
/// up, and the window does not move inwards by the band's thickness and
/// back.
pub fn work_area_excludes(work_area: Rect, band: Rect, edge: DockPosition) -> bool {
    match edge {
        DockPosition::Bottom => work_area.bottom() <= band.y,
        DockPosition::Top => work_area.y >= band.bottom(),
        DockPosition::Left => work_area.x >= band.right(),
        DockPosition::Right => work_area.right() <= band.x,
    }
}

/// How far (CSS px) a panel may reach past its window's sides across the
/// edge and still count as inside it ([`reserved_thickness`]): the page
/// rounds the panel outwards to whole CSS px, and the app layer rounds the
/// window's viewport to the nearest CSS px.
pub const PANEL_SLACK_CSS: i32 = 1;

/// The thickness to reserve for a window of `window` size (width, height)
/// whose resting panel is `panel` (in the same units, relative to the
/// window): from the panel's far side to the window's side on `edge`, which
/// is the panel's thickness plus its margin to the edge. Clamped to the
/// window; `Some(0)` for an empty panel.
///
/// `None` when the panel does not lie inside the window across `edge`
/// (within [`PANEL_SLACK_CSS`]): a rect the page measured in a viewport of
/// another size, which the app layer reads against the window's size when
/// it handles the report, after a resize the page had not seen yet (the
/// window shrinking after a drop or a smaller icon size). Its
/// distance to the edge side says nothing about the band: on `bottom` and
/// `right` it would be 0 or less, which gives the band back at once, only
/// for the page's next report to take it again. The caller keeps the
/// thickness it had. Pure.
pub fn reserved_thickness(window: (i32, i32), panel: Rect, edge: DockPosition) -> Option<i32> {
    if panel.width <= 0 || panel.height <= 0 {
        return Some(0);
    }
    let (width, height) = window;
    let (start, end, limit) = if is_vertical(edge) {
        (panel.x, panel.right(), width)
    } else {
        (panel.y, panel.bottom(), height)
    };
    if start < -PANEL_SLACK_CSS || end > limit.saturating_add(PANEL_SLACK_CSS) {
        return None;
    }
    let distance = match edge {
        DockPosition::Bottom | DockPosition::Right => limit.saturating_sub(start),
        DockPosition::Top | DockPosition::Left => end,
    };
    Some(distance.clamp(0, limit.max(0)))
}
