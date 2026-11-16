//! The native backdrop window's decisions: the build check, the test
//! switches, the window region and the material.
//!
//! `windows::backdrop` reads the build number and the environment, creates
//! the window and calls `CreateRoundRectRgn` / `DwmSetWindowAttribute`; what
//! it decides with those facts is here, over plain strings and integers, so
//! it is unit-tested on every host. The `DWMSBT_*` values mirror
//! `dwmapi.h`; `windows/backdrop.rs` asserts at compile time that they equal
//! the SDK values.

use crate::BackdropMaterial;

/// The first build with `DWMWA_SYSTEMBACKDROP_TYPE` (`DWM_SYSTEMBACKDROP_TYPE`
/// documentation: Windows 11 version 22H2).
pub const MIN_BUILD: u32 = 22621;

/// How far the window region stays inside the window on every side, so the
/// region's stair steps sit under the panel's 1 px hairline.
pub const REGION_INSET: i32 = 1;

/// `DWMSBT_MAINWINDOW`: Mica.
pub const DWMSBT_MAINWINDOW: u32 = 2;
/// `DWMSBT_TRANSIENTWINDOW`: Acrylic.
pub const DWMSBT_TRANSIENTWINDOW: u32 = 3;

/// `CurrentBuildNumber`'s text as a number (`"26100"` → 26100).
pub fn parse_build(text: &str) -> Option<u32> {
    text.trim().parse().ok()
}

/// The window variant for testing (see `windows::backdrop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variant {
    /// `WS_EX_LAYERED | WS_EX_TRANSPARENT` (default) or an ordinary window.
    pub layered: bool,
    /// `DwmExtendFrameIntoClientArea(−1)` (default) or not.
    pub extend_frame: bool,
}

impl Variant {
    /// From the values of `STEM_BACKDROP_LAYERED` and
    /// `STEM_BACKDROP_EXTEND_FRAME` (`None` when unset), each by [`flag`]
    /// with `true` as the default.
    pub fn from_switches(layered: Option<&str>, extend_frame: Option<&str>) -> Self {
        Self {
            layered: flag(layered, true),
            extend_frame: flag(extend_frame, true),
        }
    }

    /// The variant as the `applied` log line names it.
    pub fn describe(self) -> String {
        format!(
            "{}, frame {}",
            if self.layered {
                "layered click-through"
            } else {
                "not layered"
            },
            if self.extend_frame {
                "extended"
            } else {
                "not extended"
            }
        )
    }
}

/// An environment switch: `"0"` is off, `"1"` is on, anything else (or
/// nothing) is `default`.
pub fn flag(value: Option<&str>, default: bool) -> bool {
    match value.map(str::trim) {
        Some("0") => false,
        Some("1") => true,
        _ => default,
    }
}

/// Name, `DWM_SYSTEMBACKDROP_TYPE` value and constant name of a material.
pub fn material_attribute(material: BackdropMaterial) -> (&'static str, u32, &'static str) {
    match material {
        BackdropMaterial::Acrylic => ("Acrylic", DWMSBT_TRANSIENTWINDOW, "DWMSBT_TRANSIENTWINDOW"),
        BackdropMaterial::Mica => ("Mica", DWMSBT_MAINWINDOW, "DWMSBT_MAINWINDOW"),
    }
}

/// Arguments of `CreateRoundRectRgn` (window-relative; `right` and `bottom`
/// exclusive, as for `CreateRectRgn`), or of `CreateRectRgn` when `ellipse`
/// is 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionArgs {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    /// Width and height of the corner ellipse: twice the region's radius.
    pub ellipse: i32,
}

/// The region of a backdrop window `width` × `height` physical px under a
/// panel with corners of `radius` px: the window inset by [`REGION_INSET`]
/// on every side, with corners concentric to the panel's (radius minus the
/// inset), never rounder than the inset box allows. `None` when nothing is
/// left.
pub fn region_args(width: i32, height: i32, radius: u32) -> Option<RegionArgs> {
    let right = width.saturating_sub(REGION_INSET);
    let bottom = height.saturating_sub(REGION_INSET);
    let inner_width = right.saturating_sub(REGION_INSET);
    let inner_height = bottom.saturating_sub(REGION_INSET);
    if inner_width <= 0 || inner_height <= 0 {
        return None;
    }
    let radius = i32::try_from(radius)
        .unwrap_or(i32::MAX)
        .saturating_sub(REGION_INSET)
        .max(0);
    let ellipse = radius.saturating_mul(2).min(inner_width).min(inner_height);
    Some(RegionArgs {
        left: REGION_INSET,
        top: REGION_INSET,
        right,
        bottom,
        ellipse,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_number_parses_from_the_registry_text() {
        assert_eq!(parse_build("26100"), Some(26100));
        assert_eq!(parse_build(" 22621 "), Some(22621));
        assert_eq!(parse_build(""), None);
        assert_eq!(parse_build("22621.1"), None);
    }

    #[test]
    fn a_switch_is_off_only_for_0_and_on_only_for_1() {
        assert!(flag(None, true));
        assert!(!flag(Some("0"), true));
        assert!(!flag(Some(" 0 "), true));
        assert!(flag(Some("1"), false));
        assert!(flag(Some("no"), true));
        assert!(!flag(Some("yes"), false));
    }

    #[test]
    fn the_region_is_inset_by_one_pixel_with_concentric_corners() {
        // A 960 x 96 panel with 33 px corners (22 css at 150 %).
        assert_eq!(
            region_args(960, 96, 33),
            Some(RegionArgs {
                left: 1,
                top: 1,
                right: 959,
                bottom: 95,
                ellipse: 64,
            })
        );
        // Square corners: a plain rectangle.
        assert_eq!(region_args(100, 50, 0).map(|r| r.ellipse), Some(0));
        assert_eq!(region_args(100, 50, 1).map(|r| r.ellipse), Some(0));
    }

    #[test]
    fn the_corner_never_exceeds_the_inset_box_and_a_tiny_panel_has_none() {
        // A radius larger than half the height gives a pill, not a bulge.
        assert_eq!(region_args(400, 40, 64).map(|r| r.ellipse), Some(38));
        assert_eq!(region_args(400, 40, u32::MAX).map(|r| r.ellipse), Some(38));
        assert_eq!(region_args(2, 40, 8), None);
        assert_eq!(region_args(40, 2, 8), None);
        assert_eq!(region_args(0, 0, 0), None);
        assert_eq!(region_args(-5, 40, 0), None);
        assert_eq!(region_args(3, 3, 8).map(|r| r.ellipse), Some(1));
    }

    #[test]
    fn the_materials_map_to_the_documented_backdrop_types() {
        assert_eq!(material_attribute(BackdropMaterial::Acrylic).1, 3);
        assert_eq!(material_attribute(BackdropMaterial::Mica).1, 2);
    }
}
