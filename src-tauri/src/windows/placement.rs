//! The placement rule of a dock window (`docs/ipc/sizing-protocol.md`)
//! and the conversion of CSS px to physical px. Pure.

use stem_core::Rect;
use stem_core::settings::DockPosition;

/// What the UI may report along the edge (the length, informational), in
/// CSS px; anything else is clamped ([`clamp_content`]).
pub(super) const LENGTH_RANGE: (f64, f64) = (64.0, 8192.0);
/// What the UI may ask for across the edge (the thickness), in CSS px.
// The maximum is generous on purpose: during a drag-to-reorder the UI asks
// for a very thick window so the lifted icon can go anywhere on screen;
// the work area clamps it further.
pub(super) const THICKNESS_RANGE: (f64, f64) = (32.0, 4096.0);

/// CSS px → physical px, rounded outwards (left/top down, right/bottom up)
/// so a region never loses an edge pixel to rounding. Pure.
pub fn scale_rect(rect: Rect, scale: f64) -> Rect {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let left = (f64::from(rect.x) * scale).floor();
    let top = (f64::from(rect.y) * scale).floor();
    let right = (f64::from(rect.right()) * scale).ceil();
    let bottom = (f64::from(rect.bottom()) * scale).ceil();
    let clamp = |v: f64| v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    Rect::new(
        clamp(left),
        clamp(top),
        clamp(right - left),
        clamp(bottom - top),
    )
}

/// Whether a dock on `edge` is vertical (`left`, `right`: its length runs
/// down the screen) rather than horizontal (`bottom`, `top`). Pure.
pub fn is_vertical(edge: DockPosition) -> bool {
    matches!(edge, DockPosition::Left | DockPosition::Right)
}

/// The dock window's rectangle on `edge` of `work_area`, in physical
/// pixels: flush with that edge, spanning the work area's full length along
/// it (its width on `bottom` and `top`, its height on `left` and `right`),
/// and `thickness` pixels across it, clamped to the work area and never
/// empty (a degenerate work area still gives a 1×1 window flush with its
/// edge). The placement rule every dock window follows, unit-tested below.
pub fn edge_rect(work_area: Rect, edge: DockPosition, thickness: i32) -> Rect {
    let width = work_area.width.max(1);
    let height = work_area.height.max(1);
    match edge {
        DockPosition::Bottom => {
            let thickness = thickness.clamp(1, height);
            Rect::new(
                work_area.x,
                work_area.bottom() - thickness,
                width,
                thickness,
            )
        }
        DockPosition::Top => {
            let thickness = thickness.clamp(1, height);
            Rect::new(work_area.x, work_area.y, width, thickness)
        }
        DockPosition::Left => {
            let thickness = thickness.clamp(1, width);
            Rect::new(work_area.x, work_area.y, thickness, height)
        }
        DockPosition::Right => {
            let thickness = thickness.clamp(1, width);
            Rect::new(
                work_area.right() - thickness,
                work_area.y,
                thickness,
                height,
            )
        }
    }
}

/// Physical window rectangle for a content size in CSS px on `edge`: the
/// thickness is `css_height` on a horizontal dock and `css_width` on a
/// vertical one, scaled by `scale` (rounded to the nearest pixel) and laid
/// on the edge by [`edge_rect`]; the other value does not count. Pure.
pub fn plan_dock_rect(
    work_area: Rect,
    scale: f64,
    css_width: f64,
    css_height: f64,
    edge: DockPosition,
) -> Rect {
    let thickness = if is_vertical(edge) {
        css_width
    } else {
        css_height
    };
    edge_rect(work_area, edge, physical_px(thickness, scale))
}

/// `work_area` made at least `length` physical pixels long along `edge`
/// (wider on `bottom` and `top`, taller on `left` and `right`), its origin
/// kept: where no monitor list is known, the content's length is the lower
/// bound, so the panel is never cut. Pure.
pub fn with_min_length(work_area: Rect, edge: DockPosition, length: i32) -> Rect {
    let mut grown = work_area;
    if is_vertical(edge) {
        grown.height = grown.height.max(length);
    } else {
        grown.width = grown.width.max(length);
    }
    grown
}

/// The thickness of a window of physical `size` (width, height) placed on
/// `placed` last: its width on a vertical edge, its height on a horizontal
/// one, and its height before its first placement (`None`: the window is
/// at [`INITIAL_SIZE`], whose height is the initial thickness on any edge).
/// What the window keeps when it is laid out before its UI reported a
/// content size. Pure.
pub fn thickness_of(size: (u32, u32), placed: Option<DockPosition>) -> i32 {
    let across = match placed {
        Some(edge) if is_vertical(edge) => size.0,
        _ => size.1,
    };
    i32::try_from(across).unwrap_or(i32::MAX)
}

/// The rect (physical px) a dock window is laid out at on `edge` of
/// `work_area`, every time it is sized or placed ([`layout`]):
/// - with `content`, the UI's latest content size (CSS px), taken across
///   `edge` by [`DockContent::on_edge`] (a size reported on an edge of the
///   other orientation keeps its thickness) and scaled by `scale`
///   ([`plan_dock_rect`]); where `monitor_known` is `false` (`work_area` is
///   Tauri's own view, the platform listed no monitors yet) the content's
///   length is the lower bound of the window's length, so the panel is
///   never cut (`docs/ipc/sizing-protocol.md`);
/// - before the UI's first report (`content` is `None`), at the window's
///   current thickness: the side of its physical `size` that
///   [`thickness_of`] picks for the edge it was `placed` on last (after a
///   DPI change, the size tao gave it to keep its logical size).
///
/// Pure.
pub fn relayout_rect(
    work_area: Rect,
    monitor_known: bool,
    edge: DockPosition,
    content: Option<DockContent>,
    scale: f64,
    size: (u32, u32),
    placed: Option<DockPosition>,
) -> Rect {
    let Some(content) = content else {
        return edge_rect(work_area, edge, thickness_of(size, placed));
    };
    let (width, height) = content.on_edge(edge);
    let work_area = if monitor_known {
        work_area
    } else {
        let length = if is_vertical(edge) { height } else { width };
        with_min_length(work_area, edge, physical_px(length, scale))
    };
    plan_dock_rect(work_area, scale, width, height, edge)
}

/// One window call of a placement ([`placement_steps`]), in physical px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceStep {
    /// A new size; the top-left corner stays (`set_size`).
    Size { width: i32, height: i32 },
    /// A new top-left corner; the size stays (`set_position`).
    Move { x: i32, y: i32 },
}

/// The window calls that take a dock window from `current` (its rect now)
/// to `target`, such that every rect it passes through lies inside
/// `current` or inside `target`: shrink to the smaller of the two sizes on
/// each axis where it stands (inside `current`), move (inside `target`),
/// grow to `target`; a call that would change nothing is left out, and a
/// window that keeps its top-left corner is resized in one call. The new
/// size at the old place, which a resize-then-move would pass through, can
/// stick out of both on a move between edges (a right dock turning into a
/// top one, 1920 px wide at the old `x`, lies mostly on the monitor to its
/// right), and Windows changes the DPI of a window whose larger part lands
/// on a monitor with another scale (`WM_DPICHANGED`, which tao answers by
/// resizing the window), then changes it back on the move. With
/// `current` unknown (`None`), resize then move. Pure.
pub fn placement_steps(current: Option<Rect>, target: Rect) -> Vec<PlaceStep> {
    let resize = PlaceStep::Size {
        width: target.width,
        height: target.height,
    };
    let reposition = PlaceStep::Move {
        x: target.x,
        y: target.y,
    };
    let Some(current) = current else {
        return vec![resize, reposition];
    };
    let same_size = (current.width, current.height) == (target.width, target.height);
    if (current.x, current.y) == (target.x, target.y) {
        return if same_size { Vec::new() } else { vec![resize] };
    }
    let interim = (
        current.width.min(target.width).max(1),
        current.height.min(target.height).max(1),
    );
    let mut steps = Vec::with_capacity(3);
    if interim != (current.width, current.height) {
        steps.push(PlaceStep::Size {
            width: interim.0,
            height: interim.1,
        });
    }
    steps.push(reposition);
    if interim != (target.width, target.height) {
        steps.push(resize);
    }
    steps
}

/// The content size a dock window's UI reported through
/// `set_dock_content_size` (CSS px, clamped by [`clamp_content`]), with the
/// edge in effect when it was reported, kept per window
/// ([`DockWindowState`]) to lay the window out again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockContent {
    pub width: f64,
    pub height: f64,
    pub edge: DockPosition,
}

impl DockContent {
    /// The content as `(width, height)` for a dock window on `edge`: as
    /// reported while `edge` has the orientation it was reported on,
    /// transposed otherwise, so the thickness stays the thickness (and the
    /// length the length) after an edge change until the UI reports for the
    /// new edge (`docs/ipc/sizing-protocol.md`, step 5). Pure.
    pub fn on_edge(self, edge: DockPosition) -> (f64, f64) {
        if is_vertical(edge) == is_vertical(self.edge) {
            (self.width, self.height)
        } else {
            (self.height, self.width)
        }
    }
}

/// A reported content size (CSS px) clamped for a dock on `edge`: the value
/// across the edge (the height on a horizontal dock, the width on a
/// vertical one) to [`THICKNESS_RANGE`], the one along it to
/// [`LENGTH_RANGE`]. Returns `(width, height)`. Pure.
pub fn clamp_content(edge: DockPosition, width: f64, height: f64) -> (f64, f64) {
    let length = |v: f64| v.clamp(LENGTH_RANGE.0, LENGTH_RANGE.1);
    let thickness = |v: f64| v.clamp(THICKNESS_RANGE.0, THICKNESS_RANGE.1);
    if is_vertical(edge) {
        (thickness(width), length(height))
    } else {
        (length(width), thickness(height))
    }
}

/// CSS px → physical px, rounded to the nearest pixel (never negative).
pub(super) fn physical_px(css: f64, scale: f64) -> i32 {
    ((css * scale).round().max(0.0)).min(f64::from(i32::MAX)) as i32
}
