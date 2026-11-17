//! User settings: the stored document, its defaults, validation (clamping)
//! and version migration. The Tauri app chooses where the file lives
//! (`settings.json` in its config folder), `stem-service` writes it after
//! every change, and this module defines what it contains.
//!
//! The document and each object in it use `#[serde(default)]`, so that a
//! file written by an older Stem or edited by hand still loads: a missing
//! key takes its default. The entries of `pinned` and `hiddenApps` are the
//! exception: each needs its `id`, and a pin also needs a `launch` object
//! with the keys its `kind` requires. Unknown fields are ignored.
//!
//! The doc comments of the settings types are user-facing text. ts-rs copies
//! them into the TypeScript bindings in `ui/src/lib/ipc/generated/`, and
//! `tests/settings_reference.rs` builds the settings reference,
//! `docs/SETTINGS.md`, from them.

mod appearance;
mod apps;
mod behavior;
mod document;
mod example;
mod items;
mod layout;
mod system;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use appearance::{Appearance, Backdrop, ThemeMode, ThemePreset};
pub use apps::{HiddenApp, PinnedApp};
pub use behavior::{Behavior, DEFAULT_FOCUS_SHORTCUT, DEFAULT_TOGGLE_SHORTCUT, ScrollAction};
pub use items::FixedItems;
pub use layout::{AutoHide, AutoHideMode, DockPosition, Magnification, MonitorMode};
pub use system::SystemIntegration;

/// The document format version this build writes. Increase it when a
/// breaking change needs a migration step in [`Settings::migrate`].
pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings file is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("settings version {found} is newer than supported version {supported}")]
    TooNew { found: u32, supported: u32 },
}

/// The whole settings document, as stored in `settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Format version of the document. Stem writes the version it
    /// supports. A document with a lower version is upgraded when it loads,
    /// and a document with a higher version is unreadable.
    pub version: u32,
    /// Screen edge the dock is on. A change applies at once: the dock
    /// windows move to the new edge.
    pub position: DockPosition,
    /// Base icon size in CSS pixels, before magnification (24..=128).
    pub icon_size: u32,
    /// Magnification of icons near the cursor.
    pub magnification: Magnification,
    /// When and how the dock hides itself.
    pub auto_hide: AutoHide,
    /// Which monitors get a dock. A change applies at once: Stem opens or
    /// closes the docks of the other monitors, and the primary monitor's
    /// dock stays.
    pub monitors: MonitorMode,
    /// Light or dark colours for the dock and the settings window.
    pub theme: ThemeMode,
    /// How the dock looks: theme preset, opacity, corner radius, backdrop,
    /// tooltips and the launch bounce.
    pub appearance: Appearance,
    /// Which fixed items (the items that are not apps) the dock shows.
    pub items: FixedItems,
    /// What clicks, the mouse wheel and the global shortcuts do, and which
    /// windows the dock lists.
    pub behavior: Behavior,
    /// Pinned apps, in dock order: left to right, or top to bottom on a
    /// dock at the left or right edge. Entries whose id is empty or only
    /// whitespace are dropped, and of several entries with the same id only
    /// the first stays.
    pub pinned: Vec<PinnedApp>,
    /// Apps the dock does not show while they run. *Hide from Stem* in a
    /// running app's menu adds one, and *Show* on the settings page removes
    /// it. A listed app gets no item on any dock, but its windows still
    /// count for everything else: the foreground window, fullscreen
    /// detection and the overlap test of auto-hide. A pinned app is never
    /// hidden, so an entry whose id is pinned is dropped. Entries whose id is
    /// empty or only whitespace are dropped too, and of several entries with
    /// the same id only the first stays.
    pub hidden_apps: Vec<HiddenApp>,
    /// Windows integration: start at sign-in, the stock taskbar, reserved
    /// screen space and the log file.
    pub system: SystemIntegration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            position: DockPosition::Bottom,
            icon_size: 48,
            magnification: Magnification::default(),
            auto_hide: AutoHide::default(),
            monitors: MonitorMode::Primary,
            theme: ThemeMode::System,
            appearance: Appearance::default(),
            items: FixedItems::default(),
            behavior: Behavior::default(),
            pinned: Vec::new(),
            hidden_apps: Vec::new(),
            system: SystemIntegration::default(),
        }
    }
}

impl Settings {
    /// Whether replacing `self` by `other` changes where the dock windows
    /// are or which exist: the screen edge (the app moves every window in
    /// place) or the set of monitors that get a dock (windows are
    /// created or closed). Everything else restyles them in place.
    pub fn diff_requires_window_rebuild(&self, other: &Settings) -> bool {
        self.position != other.position || self.monitors != other.monitors
    }
}
