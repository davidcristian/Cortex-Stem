//! Facts about this installation: the version, the app's own files and
//! folders, and the user's stylesheet.

use serde::{Deserialize, Serialize};

/// Facts about this installation for the settings page's *About* section:
/// the answer to `get_app_info`. Paths are shown, never parsed, by
/// the UI; `open_app_folder` opens the folders behind them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// The app's package version (`0.1.0`).
    pub version: String,
    /// The settings file the app reads and writes, as a path; or a short
    /// note in parentheses when nothing is saved this run (the mock
    /// platform, a profile that could not be used).
    pub settings_path: String,
    /// The log file (`stem.log` in the app's log folder), or `""` while file
    /// logging is off (`system.fileLogging` at startup).
    pub log_path: String,
    /// The platform implementation's name: `"windows"`, or `"mock"` in a
    /// demo run.
    pub platform: String,
}

/// A folder of the app's own that `open_app_folder` opens in Explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum AppFolder {
    /// The log folder (`AppInfo::log_path`'s folder), or its parent while
    /// none exists yet.
    Logs,
    /// The settings file's folder (`AppInfo::settings_path`'s folder).
    Settings,
}

/// The user's stylesheet: `dock.css` in the settings file's folder,
/// as the app read it at start or at the last `reload_user_css`. The answer
/// to `get_user_css` and `reload_user_css`, and the payload of
/// `dock://user-css`. The dock page puts `css` in one `<style>` element
/// after its own styles; `docs/THEMING.md` lists the variables it supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct UserCss {
    /// Where the file is, as a path shown (never parsed) by the UI; or a
    /// short note in parentheses when nothing is saved this run (the mock
    /// platform, a profile that could not be used), which has no file.
    pub path: String,
    /// The file's text, a leading byte order mark dropped; `null` when the
    /// file does not exist, cannot be read, is not UTF-8 or is larger than
    /// 64 KiB (`problem` says which, and so does the log).
    pub css: Option<String>,
    /// Why `css` is `null` (IPC v1.29); `null` when it is not.
    pub problem: Option<UserCssProblem>,
}

/// Why the app has no text from the user's `dock.css` ([`UserCss::problem`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum UserCssProblem {
    /// There is no such file: the normal case until the user makes one
    /// (*Edit dock.css* writes a template). Also what a run that saves no
    /// files says, which has no `dock.css` at all.
    NotFound,
    /// The file is there but could not be opened or read (no permission,
    /// locked, a folder of that name, a disk error).
    Unreadable,
    /// The file is not UTF-8 text (UTF-16 from Notepad's *Unicode*, say).
    NotUtf8,
    /// The file is larger than 64 KiB, the most the app reads.
    TooLarge,
}
