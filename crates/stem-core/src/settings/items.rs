//! The fixed items: the items of the dock that are not apps.

use serde::{Deserialize, Serialize};

/// Which fixed items (the items that are not apps) the dock shows. The
/// items at the left come before the apps and the items at the right come
/// after them; on a dock at the left or right edge, left means the top and
/// right means the bottom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct FixedItems {
    /// The Start item at the left, which opens the Start menu.
    pub launcher: bool,
    /// The Task View item at the left.
    pub task_view: bool,
    /// The Show Desktop item at the left.
    pub show_desktop: bool,
    /// The Quick Settings item at the right, before the Recycle Bin. A
    /// click opens what Win+A opens.
    pub quick_settings: bool,
    /// The Notification Center item at the right, before the Recycle Bin.
    /// A click opens what Win+N opens.
    pub notification_center: bool,
    /// The volume item at the right, after Notification Center. It shows
    /// the volume of the default output device. A click opens what Win+A
    /// opens, and the mouse wheel over it changes the volume, whatever
    /// `behavior.scrollAction` is set to.
    pub volume: bool,
    /// The battery item at the right, after the volume. It appears only on
    /// a PC with a battery. A click opens what Win+A opens.
    pub battery: bool,
    /// The clock item at the right, after the battery. A click opens what
    /// Win+N opens.
    pub clock: bool,
    /// The Recycle Bin item, last at the right.
    pub recycle_bin: bool,
    /// Draw thin separators between the item groups: the items at the
    /// left, the pinned apps, the other running apps and the items at the
    /// right.
    pub separators: bool,
}

impl Default for FixedItems {
    fn default() -> Self {
        Self {
            launcher: true,
            task_view: false,
            show_desktop: false,
            quick_settings: false,
            notification_center: false,
            volume: false,
            battery: false,
            clock: false,
            recycle_bin: true,
            separators: true,
        }
    }
}
