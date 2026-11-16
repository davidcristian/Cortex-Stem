//! What the platform reports about the system: rectangles in physical
//! pixels, the monitors, the theme and the Recycle Bin.

use serde::{Deserialize, Serialize};

use super::MonitorId;

/// Axis-aligned rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// A display attached to the system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: MonitorId,
    pub name: String,
    /// Full bounds of the monitor in the virtual desktop coordinate space.
    pub bounds: Rect,
    /// Bounds minus reserved areas (stock taskbar, app bars).
    pub work_area: Rect,
    /// DPI scale factor (1.0 = 96 DPI, 1.5 = 144 DPI, ...).
    pub scale_factor: f64,
    pub is_primary: bool,
}

/// Current OS theme facts, emitted as `dock://theme`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ThemeInfo {
    /// Accent colour as `#RRGGBB`.
    pub accent: String,
    pub dark: bool,
    pub high_contrast: bool,
    /// Whether the OS has transparency effects enabled.
    pub transparency: bool,
}

impl Default for ThemeInfo {
    fn default() -> Self {
        Self {
            accent: "#0078D4".to_owned(),
            dark: true,
            high_contrast: false,
            transparency: true,
        }
    }
}

/// Recycle Bin contents summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RecycleBinState {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub item_count: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size_bytes: u64,
}

impl RecycleBinState {
    pub fn is_empty(&self) -> bool {
        self.item_count == 0
    }
}
