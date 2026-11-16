//! Opaque identifiers of windows, apps, icons and monitors, and the path
//! form that an app id uses.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Opaque identifier of a top-level window. On Windows this is the `HWND`
/// value, but nothing outside `stem-platform` may rely on that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
pub struct WindowId(#[cfg_attr(feature = "ts", ts(type = "number"))] pub u64);

/// Stable identity of an application, used to group windows and to match
/// pinned entries with running windows. See [`AppId::for_exe`] and
/// [`AppId::for_app_user_model_id`] for the canonical forms.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
pub struct AppId(pub String);

impl AppId {
    /// Identity for a classic desktop application identified by its executable.
    /// The path is normalised (lower-cased, forward slashes) so that the same
    /// binary always yields the same id regardless of how it was spelled.
    pub fn for_exe(path: &Path) -> Self {
        Self(format!("exe:{}", normalize_path(path)))
    }

    /// Identity for an application that declares an AppUserModelID (Store/UWP
    /// apps and desktop apps that opt in, e.g. browsers with profiles).
    pub fn for_app_user_model_id(aumid: &str) -> Self {
        Self(format!("aumid:{}", aumid.trim()))
    }

    /// Last-resort identity based on the window class name.
    pub fn for_window_class(class_name: &str) -> Self {
        Self(format!("class:{}", class_name.trim().to_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Normalise a filesystem path for identity purposes.
pub fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Opaque key the platform can turn into PNG bytes (see `Platform::icon_png`).
/// Keys starting with `shell:` are rendered by the frontend from built-in
/// vector art (`shell:start`, `shell:task-view`, `shell:show-desktop`,
/// `shell:quick-settings`, `shell:notification-center`,
/// `shell:recycle-bin-empty`, `shell:recycle-bin-full`; the clock, battery
/// and volume faces `shell:clock`, `shell:battery` and `shell:volume` are
/// drawn from live data).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
pub struct IconKey(pub String);

impl IconKey {
    pub fn shell(name: &str) -> Self {
        Self(format!("shell:{name}"))
    }

    pub fn is_shell(&self) -> bool {
        self.0.starts_with("shell:")
    }
}

/// Opaque monitor identifier (stable across sessions where the OS allows).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
pub struct MonitorId(pub String);
