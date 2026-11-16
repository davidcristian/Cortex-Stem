//! The pure rules of a band: whether the settings ask for bands, what a
//! dock window asks for, and whether a band fits a request.

use stem_core::settings::{AutoHideMode, DockPosition};
use stem_core::{Rect, Settings};
use stem_platform::{AppBarGrant, AppBarRequest};

/// What decides a dock window's request ([`request_for`]); gathered by
/// `windows::app_bar_inputs` on the main thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarInputs {
    /// [`reservation_wanted`] as in effect.
    pub enabled: bool,
    /// Whether the window is shown (not hidden by the tray or the toggle).
    pub visible: bool,
    /// The bounds of the monitor the window was placed on, if known.
    pub monitor: Option<Rect>,
    /// The edge the window was placed on last.
    pub placed: Option<DockPosition>,
    /// The edge in effect (`settings.position`).
    pub edge: DockPosition,
    /// From the resting panel's far side to the viewport's edge side, CSS
    /// px, from the page's latest panel rect for the edge the window is
    /// placed on, read against the viewport that report carries.
    pub reserve_css: Option<i32>,
    /// The window's scale factor.
    pub scale: f64,
}

/// Whether the settings ask for bands: `system.reserveScreenSpace` on and
/// `autoHide.mode` off. Pure.
pub fn reservation_wanted(settings: &Settings) -> bool {
    settings.system.reserve_screen_space && settings.auto_hide.mode == AutoHideMode::Off
}

/// The band a dock window asks for, or `None`: nothing while bands are not
/// wanted, while the window is hidden, before it is placed on the edge in
/// effect or on a known monitor, or before its page reported a panel for
/// that edge. The thickness is `reserve_css` scaled and rounded up, so the
/// band never stops short of the panel. Pure.
pub fn request_for(inputs: &BarInputs) -> Option<AppBarRequest> {
    if !inputs.enabled || !inputs.visible || inputs.placed != Some(inputs.edge) {
        return None;
    }
    let monitor = inputs.monitor?;
    let css = inputs.reserve_css.filter(|&css| css > 0)?;
    let scale = if inputs.scale.is_finite() && inputs.scale > 0.0 {
        inputs.scale
    } else {
        1.0
    };
    let thickness = (f64::from(css) * scale)
        .ceil()
        .clamp(1.0, f64::from(i32::MAX)) as i32;
    Some(AppBarRequest {
        monitor,
        edge: inputs.edge,
        thickness,
    })
}

/// Whether `grant` is a band for the place `request` asks for: the same
/// edge of the same monitor (the thickness may still be on its way). Pure.
pub fn grant_fits(grant: &AppBarGrant, request: &AppBarRequest) -> bool {
    grant.request.edge == request.edge && grant.request.monitor == request.monitor
}
