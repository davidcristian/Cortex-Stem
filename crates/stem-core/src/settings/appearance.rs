//! How the dock looks: light or dark colours, the theme preset, the
//! backdrop and the panel's shape.

use serde::{Deserialize, Serialize};

/// Light or dark colours for the dock and the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    /// Follow the Windows app mode, light or dark (`ThemeInfo.dark`).
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// What is drawn behind the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Backdrop {
    /// A translucent CSS background only. It works on every Windows version
    /// and on every dock.
    Css,
    /// The acrylic material of DWM (the Desktop Window Manager, which draws
    /// the windows on screen) behind the panel. Experimental, and the
    /// settings page does not offer it. On Windows 11 22H2 or later, a
    /// native window under the primary dock's panel shows the material (a
    /// prototype). On older Windows versions, under a contrast theme and on
    /// every secondary dock, the panel draws the `css` backdrop instead.
    Acrylic,
    /// The mica material of DWM behind the panel. Experimental, and the
    /// settings page does not offer it. On Windows 11 22H2 or later, a
    /// native window under the primary dock's panel shows the material (a
    /// prototype). On older Windows versions, under a contrast theme and on
    /// every secondary dock, the panel draws the `css` backdrop instead.
    Mica,
}

/// The dock's theme preset: how the panel is drawn. Every preset takes
/// light or dark from `Settings::theme`, and its running dots use the
/// Windows accent colour. The user's `dock.css`, in the folder of the
/// settings file, can change any preset further through the variables
/// that `docs/THEMING.md` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ThemePreset {
    /// A translucent, blurred panel with a thin outline and a soft shadow
    /// (the default). `opacity` sets how translucent it is.
    Standard,
    /// An opaque panel without blur, with a stronger outline and a tighter
    /// shadow. It is easy to read over any wallpaper, and the best choice
    /// while Windows' *Transparency effects* are off.
    Solid,
    /// No panel fill, outline or shadow: only the icons, the running dots
    /// and the separators.
    Minimal,
}

/// How the dock looks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    /// Theme preset of the panel. A change restyles the dock at once.
    pub theme: ThemePreset,
    /// Panel background opacity of the `standard` theme (0.0..=1.0). The
    /// `solid` panel is always opaque, and the `minimal` one has no fill.
    pub opacity: f32,
    /// Panel corner radius in CSS pixels (0..=64).
    pub corner_radius: u32,
    /// Backdrop material behind the panel.
    pub backdrop: Backdrop,
    /// Show an item's name in a tooltip while the cursor is over the item
    /// or the item has the keyboard focus.
    pub tooltips: bool,
    /// Bounce the icon of an app that the dock launched while the app
    /// starts. No icon bounces while Windows' *Animation effects* are off.
    pub bounce_on_launch: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemePreset::Standard,
            opacity: 0.72,
            corner_radius: 22,
            backdrop: Backdrop::Css,
            tooltips: true,
            bounce_on_launch: true,
        }
    }
}
