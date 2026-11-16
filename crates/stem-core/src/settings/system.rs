//! Windows integration settings.

use serde::{Deserialize, Serialize};

/// Windows integration settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct SystemIntegration {
    /// Start Stem when the user signs in.
    pub autostart: bool,
    /// Hide the stock Windows taskbar while Stem runs, by turning on
    /// Windows' own *Automatically hide the taskbar* option. Turning this
    /// off, or quitting Stem, sets that option back to what it was.
    pub hide_windows_taskbar: bool,
    /// Register each dock window as an AppBar (an application desktop
    /// toolbar: Windows takes a band at the dock's screen edge out of the
    /// work area, as it does for the stock taskbar), so that maximised
    /// windows end at the dock instead of going under it. This works only
    /// while `autoHide.mode` is `off`: an auto-hiding dock reserves
    /// nothing. A change applies at once.
    pub reserve_screen_space: bool,
    /// Write a log file, `stem.log`, in Stem's `logs` folder under
    /// `%LOCALAPPDATA%`. Stem reads this setting when it starts, so a change
    /// applies after a restart.
    pub file_logging: bool,
}

impl Default for SystemIntegration {
    fn default() -> Self {
        Self {
            autostart: false,
            hide_windows_taskbar: false,
            reserve_screen_space: false,
            file_logging: true,
        }
    }
}
