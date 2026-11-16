//! Where the dock is and how it moves: the screen edge, magnification,
//! auto-hide and the monitors that get a dock.

use serde::{Deserialize, Serialize};

/// Screen edge the dock is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum DockPosition {
    /// Bottom edge, as a row centred on it (the default).
    Bottom,
    /// Left edge, as a column centred on it. Magnified icons grow towards
    /// the middle of the screen, and the name tooltips appear beside them.
    Left,
    /// Right edge, as a column centred on it. Magnified icons grow towards
    /// the middle of the screen, and the name tooltips appear beside them.
    Right,
    /// Top edge, as a row centred on it.
    Top,
}

/// Icon magnification near the cursor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Magnification {
    /// Magnify icons as the cursor comes near them.
    pub enabled: bool,
    /// Scale of the icon directly under the cursor (1.0..=2.5).
    pub max_scale: f32,
    /// How many neighbouring icons on each side of that icon grow too
    /// (1..=6).
    pub range_icons: u32,
}

impl Default for Magnification {
    fn default() -> Self {
        Self {
            enabled: true,
            max_scale: 1.6,
            range_icons: 3,
        }
    }
}

/// When the dock hides itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum AutoHideMode {
    /// The dock stays on screen.
    Off,
    /// The dock hides whenever the cursor leaves it, and comes back when
    /// the cursor touches the dock's screen edge.
    Always,
    /// The dock hides only while a window of another program overlaps the
    /// panel, and comes back as soon as none does.
    WhenOverlapped,
}

/// Auto-hide: when the dock hides itself, and how long it waits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct AutoHide {
    /// Whether and when the dock hides. Whatever the mode, a fullscreen
    /// app hides the dock of its monitor.
    pub mode: AutoHideMode,
    /// How long the dock waits before it hides, in milliseconds
    /// (0..=5000). The wait starts when the cursor leaves the dock, or when
    /// a window starts to overlap it.
    pub delay_ms: u32,
}

impl Default for AutoHide {
    fn default() -> Self {
        Self {
            mode: AutoHideMode::Off,
            delay_ms: 250,
        }
    }
}

/// Which monitors get a dock window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum MonitorMode {
    /// One dock, on the primary monitor.
    Primary,
    /// One dock per monitor.
    All,
}
