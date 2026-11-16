//! How to start an app, and what is known about it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{AppId, IconKey};

/// How to start an application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LaunchTarget {
    /// A classic desktop executable.
    #[serde(rename_all = "camelCase")]
    Exe {
        /// Path of the executable.
        path: PathBuf,
        /// Command-line arguments, one string per argument.
        #[serde(default)]
        args: Vec<String>,
        /// Working directory to start the app in, or `null` to start it in
        /// the executable's own folder.
        #[serde(default)]
        working_dir: Option<PathBuf>,
    },
    /// A `.lnk` shortcut. Windows resolves it when Stem launches it, and
    /// Stem reads the shortcut to identify the app it starts.
    #[serde(rename_all = "camelCase")]
    Shortcut {
        /// Path of the `.lnk` file.
        path: PathBuf,
    },
    /// A packaged (Store or UWP) app, launched by its AppUserModelID.
    #[serde(rename_all = "camelCase")]
    StoreApp {
        /// The app's AppUserModelID.
        app_user_model_id: String,
    },
    /// Anything that `ShellExecute` can open, such as `ms-settings:`,
    /// `shell:AppsFolder` or a web URL.
    #[serde(rename_all = "camelCase")]
    Uri {
        /// The URI to open.
        uri: String,
    },
}

/// Resolved facts about an application: how to show it and how to start it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AppIdentity {
    pub id: AppId,
    pub display_name: String,
    pub icon_key: Option<IconKey>,
    /// `None` when the app cannot be (re)launched, e.g. identity came only
    /// from a window class.
    pub launch: Option<LaunchTarget>,
}
