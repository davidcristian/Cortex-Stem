//! What the user asks for: an action on a window, the direction of a wheel
//! notch, and the shell actions behind the fixed items.

use serde::{Deserialize, Serialize};

/// Something the user can do to a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum WindowAction {
    Activate,
    Minimize,
    Restore,
    Close,
}

/// Which way one wheel notch over a dock item went (`scroll_item`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ScrollDirection {
    /// The wheel turned towards the user (a positive `deltaY`): the window
    /// after the item's one in front.
    Next,
    /// The wheel turned away from the user: the window before it.
    Previous,
}

/// Shell-level actions behind the fixed items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ShellAction {
    OpenStartMenu,
    OpenTaskView,
    ToggleShowDesktop,
    OpenRecycleBin,
    EmptyRecycleBin,
    /// Windows' quick settings flyout, as Win+A opens it.
    OpenQuickSettings,
    /// Windows' notification center and calendar, as Win+N opens them.
    OpenNotificationCenter,
    /// The Settings app's date and time page (`ms-settings:dateandtime`).
    OpenDateTimeSettings,
    /// The Settings app's power and battery page (`ms-settings:powersleep`).
    OpenPowerSettings,
    /// The Settings app's sound page (`ms-settings:sound`, the volume
    /// item's menu).
    OpenSoundSettings,
    /// The Settings app's volume mixer (`ms-settings:apps-volume`,
    /// the volume item's menu).
    OpenVolumeMixer,
}
