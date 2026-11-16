//! A dock window's viewport in CSS px, and the panel report the page
//! measured in it, moved to the window's viewport of the moment
//! ([`reanchor_panel`]). Pure.

use stem_core::Rect;
use stem_core::settings::DockPosition;

/// A dock window's viewport in CSS px: the space the page measures in.
/// What `set_dock_panel_rect` carries as `viewport` (the page's
/// `window.innerWidth` × `window.innerHeight`, whole CSS px), and what the
/// window has now ([`CssViewport::of`] its client area, fractional at a
/// scale other than 1). The app layer's own type: no `stem-core` type
/// crosses IPC for it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct CssViewport {
    pub width: f64,
    pub height: f64,
}

impl CssViewport {
    /// A client area of `physical` px (width, height) at `scale` (1 when
    /// unusable). Pure.
    pub fn of(physical: (u32, u32), scale: f64) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        Self {
            width: f64::from(physical.0) / scale,
            height: f64::from(physical.1) / scale,
        }
    }

    /// Argument check for `set_dock_panel_rect`'s `viewport`: both sides
    /// finite and positive (`docs/ipc/report-provenance.md`). Pure.
    pub fn validate(self) -> Result<(), String> {
        let usable = |v: f64| v.is_finite() && v > 0.0;
        if usable(self.width) && usable(self.height) {
            Ok(())
        } else {
            Err(format!(
                "the viewport must have a positive, finite width and height: {self:?}"
            ))
        }
    }

    /// Rounded to whole CSS px, the space of the page's rects.
    pub(super) fn whole(self) -> (i32, i32) {
        let whole = |v: f64| {
            if v.is_finite() {
                v.round().clamp(0.0, f64::from(i32::MAX)) as i32
            } else {
                0
            }
        };
        (whole(self.width), whole(self.height))
    }
}

/// One `set_dock_panel_rect` with its provenance, kept per
/// window ([`DockWindowState`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelReport {
    /// The panel at rest, whole CSS px relative to `viewport`.
    pub rect: Rect,
    /// The edge the page had laid itself out for.
    pub edge: DockPosition,
    /// The page's viewport the rect is relative to.
    pub viewport: CssViewport,
}

impl PanelReport {
    /// The rect in a window whose viewport is `now` ([`reanchor_panel`]):
    /// where the panel rests now. As reported when the window cannot say
    /// (`None`). Pure.
    pub fn now(self, now: Option<CssViewport>) -> Rect {
        match now {
            Some(now) => reanchor_panel(self.rect, self.edge, self.viewport, now),
            None => self.rect,
        }
    }
}

/// How far (CSS px) the page's viewport and the window's may differ on an
/// axis and still count as the same ([`reanchor_panel`]): the page reports
/// whole CSS px, and the window's client area of whole physical px is
/// fractional in CSS px at a scale other than 1, so the two differ by less
/// than 1 when nothing changed. The window's thickness and length change by
/// whole CSS px at 1×, and by more than one at any larger scale.
pub const VIEWPORT_SLACK_CSS: f64 = 1.0;

/// The panel `rect` (CSS px) that the page measured in the viewport `from`,
/// laid out for `edge`, moved to where the page's layout
/// (`docs/ipc/sizing-protocol.md`: the panel anchored to the window's edge
/// side and centred along it) puts it in the viewport `to`: across the edge
/// it keeps its distance to the viewport's edge side (on `bottom` it moves
/// by the change in height, on `right` by the change in width, on `top` and
/// `left` it stays), along the edge its offset from the viewport's centre
/// (it moves by half the change in length, rounded to the nearest CSS px).
/// Its size never changes. A difference under [`VIEWPORT_SLACK_CSS`] on an
/// axis moves nothing on it. Pure.
pub fn reanchor_panel(rect: Rect, edge: DockPosition, from: CssViewport, to: CssViewport) -> Rect {
    let change = |before: f64, after: f64| {
        let d = after - before;
        if d.is_finite() && d.abs() >= VIEWPORT_SLACK_CSS {
            d
        } else {
            0.0
        }
    };
    let dw = change(from.width, to.width);
    let dh = change(from.height, to.height);
    let (dx, dy) = match edge {
        DockPosition::Bottom => (dw / 2.0, dh),
        DockPosition::Top => (dw / 2.0, 0.0),
        DockPosition::Left => (0.0, dh / 2.0),
        DockPosition::Right => (dw, dh / 2.0),
    };
    let shift = |v: i32, d: f64| {
        (f64::from(v) + d)
            .round()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    };
    Rect::new(
        shift(rect.x, dx),
        shift(rect.y, dy),
        rect.width,
        rect.height,
    )
}
