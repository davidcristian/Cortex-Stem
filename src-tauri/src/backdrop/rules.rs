//! The pure rules of the backdrop: what the panel gets behind it, where
//! the material goes and what the platform is asked for.

use stem_core::settings::{Backdrop, ThemeMode};
use stem_core::{Rect, ThemeInfo};
use stem_platform::{BackdropMaterial, DockBackdrop};

use crate::windows::CssViewport;

/// What the panel gets behind it, from the setting and the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effective {
    /// The built-in CSS look; no backdrop window.
    Css,
    /// A native material in a window under the panel.
    Native(BackdropMaterial),
    /// A material was asked for, but a contrast theme is on: no material,
    /// and the panel draws its opaque contrast look.
    ContrastTheme(BackdropMaterial),
}

/// `appearance.backdrop` under the current theme. Pure.
pub fn effective(setting: Backdrop, high_contrast: bool) -> Effective {
    let material = match setting {
        Backdrop::Css => return Effective::Css,
        Backdrop::Acrylic => BackdropMaterial::Acrylic,
        Backdrop::Mica => BackdropMaterial::Mica,
    };
    if high_contrast {
        Effective::ContrastTheme(material)
    } else {
        Effective::Native(material)
    }
}

/// Whether the material is the dark one: `settings.theme` when it forces
/// light or dark, else Windows' app theme, as for the panel itself. Pure.
pub fn dark(mode: ThemeMode, theme: &ThemeInfo) -> bool {
    match mode {
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
        ThemeMode::System => theme.dark,
    }
}

/// Where the panel rests, as the backdrop needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelPlace {
    /// Physical px of the virtual desktop, rounded inwards.
    pub rect: Rect,
    /// The window's scale factor the rect was converted at.
    pub scale: f64,
}

/// The resting panel `css` (CSS px of the viewport) in physical pixels of
/// the virtual desktop, for a window whose top-left corner is `origin`
/// (physical px) at `scale`: like `windows::panel_on_screen`, but rounded
/// **inwards** (left and top up, right and bottom down), so the material
/// never covers a pixel the panel does not. Pure.
pub fn panel_inwards(origin: (i32, i32), css: Rect, scale: f64) -> Rect {
    let scale = usable_scale(scale);
    let left = (f64::from(css.x) * scale).ceil();
    let top = (f64::from(css.y) * scale).ceil();
    let right = (f64::from(css.right()) * scale).floor();
    let bottom = (f64::from(css.bottom()) * scale).floor();
    let clamp = |v: f64| v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    Rect::new(
        origin.0.saturating_add(clamp(left)),
        origin.1.saturating_add(clamp(top)),
        clamp((right - left).max(0.0)),
        clamp((bottom - top).max(0.0)),
    )
}

/// One conversion of the primary dock's resting panel, from `windows`
/// (`push_dock_panel`): after a panel report, a move or resize of the
/// window, or a show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelConversion {
    /// The window's top-left corner, physical px of the virtual desktop.
    pub origin: (i32, i32),
    /// The UI's resting panel, CSS px, re-anchored to `viewport`:
    /// where the panel rests in the window now.
    pub css: Rect,
    /// The window's scale factor.
    pub scale: f64,
    /// The window's viewport now; `None` when the window could not say (and
    /// `css` is the UI's rect as reported).
    pub viewport: Option<CssViewport>,
}

/// How far (CSS px) a panel may reach past any side of its viewport and
/// still lie inside it: the UI rounds the panel outwards to whole CSS px,
/// and a client area of whole physical px is fractional in CSS px.
const INSIDE_SLACK: f64 = 1.5;

/// Whether the resting panel `css`, re-anchored to the window's viewport
/// `now`, lies inside it on both axes, so the material under it
/// covers only what the window shows of the panel: a panel longer than its
/// window along the edge, or one cut by a window thinner than it across the
/// edge, has none. (Earlier this also required the viewport to have
/// the size the report was measured in, because the rect was not
/// re-anchored: a resize took the material away until the UI reported the
/// panel again, which on `top` and `left` it does not do when only the
/// thickness changed.) Pure.
pub fn panel_fits(css: Rect, now: CssViewport) -> bool {
    f64::from(css.x) >= -INSIDE_SLACK
        && f64::from(css.right()) <= now.width + INSIDE_SLACK
        && f64::from(css.y) >= -INSIDE_SLACK
        && f64::from(css.bottom()) <= now.height + INSIDE_SLACK
}

/// The request for the platform: a backdrop only for a native material, a
/// known panel with an area, and a panel that is not slid out. The corner
/// radius is `appearance.cornerRadius` at the panel's scale. Pure.
pub fn wanted(
    effective: Effective,
    panel: Option<PanelPlace>,
    slid_out: bool,
    radius_css: u32,
    dark: bool,
) -> Option<DockBackdrop> {
    let Effective::Native(material) = effective else {
        return None;
    };
    let panel = panel?;
    if slid_out || panel.rect.width <= 0 || panel.rect.height <= 0 {
        return None;
    }
    let radius = (f64::from(radius_css) * usable_scale(panel.scale))
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32;
    Some(DockBackdrop {
        rect: panel.rect,
        corner_radius: radius,
        material,
        dark,
    })
}

fn usable_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests;
