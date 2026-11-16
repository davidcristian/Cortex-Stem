//! Pinned and hidden apps: the entries of `pinned` and `hiddenApps`, and
//! the methods that change those lists.

use serde::{Deserialize, Serialize};

use super::Settings;
use crate::model::{AppId, IconKey, LaunchTarget};

/// An app pinned to the dock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PinnedApp {
    /// Must match the id Stem gives the app's windows, so that the pin and
    /// the running windows share one item: `exe:` followed by the
    /// program's path in lower case with forward slashes
    /// (`AppId::for_exe`), or `aumid:` followed by the app's AppUserModelID
    /// (`AppId::for_app_user_model_id`). An AppUserModelID is the
    /// identifier Windows uses to group an app's windows on the taskbar.
    pub id: AppId,
    /// A name that replaces the app's own name on the dock, or `null` to
    /// use the app's own name.
    #[serde(default)]
    pub label: Option<String>,
    /// How to start the app when it has no window.
    pub launch: LaunchTarget,
}

/// An app the dock does not show while it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct HiddenApp {
    /// The id Stem gives the app's windows, in one of the forms of
    /// `pinned[].id`, or `class:` followed by the window class name in
    /// lower case for an app known only by its window class
    /// (`AppId::for_window_class`).
    pub id: AppId,
    /// The app's display name when it was hidden. The settings page lists
    /// the app by this name.
    #[serde(default)]
    pub name: String,
    /// The path of the app's program when it was hidden, as Windows
    /// reported it, or else the `.exe` or `.lnk` file Stem launches the app
    /// from. It keeps its original case, while an `exe:` id has the path in
    /// lower case. The settings page shows it under the name. `null` when Stem
    /// did not know a path (for an app known only by its window class, or
    /// a Store app), or when the entry has no `path` key.
    #[serde(default)]
    pub path: Option<String>,
    /// The icon the app's item showed when it was hidden. The settings page
    /// draws it beside the name. Stem never stores the icon of a single
    /// window (which an item shows when Windows does not let Stem read the
    /// app's own icon), because that icon means nothing once the window is
    /// gone; it stores the icon of the file in `path` instead, when `path`
    /// is known. `null` when Stem knew neither an icon of the app nor a
    /// path, or when the entry has no `iconKey` key.
    #[serde(default)]
    pub icon_key: Option<IconKey>,
}

impl Settings {
    pub fn is_pinned(&self, id: &AppId) -> bool {
        self.pinned.iter().any(|p| &p.id == id)
    }

    /// Append a pin. Returns `false` (and changes nothing) if already pinned.
    pub fn pin(&mut self, app: PinnedApp) -> bool {
        if self.is_pinned(&app.id) {
            return false;
        }
        self.pinned.push(app);
        true
    }

    /// Remove a pin. Returns `false` if it was not pinned.
    pub fn unpin(&mut self, id: &AppId) -> bool {
        let before = self.pinned.len();
        self.pinned.retain(|p| &p.id != id);
        self.pinned.len() != before
    }

    /// Whether `id` is listed in [`Settings::hidden_apps`].
    pub fn is_hidden(&self, id: &AppId) -> bool {
        self.hidden_apps.iter().any(|h| &h.id == id)
    }

    /// Hide an app. Returns `false` (and changes nothing) if it is
    /// already hidden or pinned: a pinned app is never hidden.
    pub fn hide(&mut self, app: HiddenApp) -> bool {
        if self.is_hidden(&app.id) || self.is_pinned(&app.id) {
            return false;
        }
        self.hidden_apps.push(app);
        true
    }

    /// Show a hidden app again. Returns `false` if it was not hidden.
    pub fn unhide(&mut self, id: &AppId) -> bool {
        let before = self.hidden_apps.len();
        self.hidden_apps.retain(|h| &h.id != id);
        self.hidden_apps.len() != before
    }

    /// Reorder pins to match `order`. Ids not in `order` keep their relative
    /// order after the listed ones; unknown ids are ignored.
    pub fn reorder_pinned(&mut self, order: &[AppId]) {
        let mut remaining = std::mem::take(&mut self.pinned);
        let mut result = Vec::with_capacity(remaining.len());
        for id in order {
            if let Some(pos) = remaining.iter().position(|p| &p.id == id) {
                result.push(remaining.remove(pos));
            }
        }
        result.extend(remaining);
        self.pinned = result;
    }
}
