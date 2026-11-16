//! The dock's snapshot, [`DockState`], and its parts: the items, the
//! per-dock facts and the readings that some items show.

use serde::{Deserialize, Serialize};

use super::{AppId, IconKey, MonitorId, WindowId, WindowInfo};

/// Kind of a dock item. Everything that is not `App` is a fixed item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum DockItemKind {
    App,
    Launcher,
    TaskView,
    ShowDesktop,
    /// Opens Windows' quick settings.
    QuickSettings,
    /// Opens Windows' notification center and calendar.
    NotificationCenter,
    /// Shows the default output device's volume from [`DockState::volume`];
    /// the wheel over it changes the volume.
    Volume,
    /// Shows the battery's charge from [`DockState::power`]; present only on
    /// a machine with a system battery.
    Battery,
    /// Shows the time and the date, drawn by the page.
    Clock,
    RecycleBin,
    Separator,
}

/// One slot in the dock, ready to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DockItem {
    /// Stable, unique key for keyed rendering (`app:<id>`, `fixed:launcher`, `sep:1`).
    pub key: String,
    pub kind: DockItemKind,
    pub app_id: Option<AppId>,
    /// Tooltip / accessible name.
    pub title: String,
    pub icon_key: Option<IconKey>,
    pub pinned: bool,
    /// Running windows of this app, in first-seen order. Empty for pinned-only
    /// apps and for fixed items.
    pub windows: Vec<WindowInfo>,
    /// One of this item's windows is the foreground window.
    pub is_active: bool,
    /// A window of this app requested attention (taskbar flash): the
    /// `needs_attention` of any of `windows`.
    pub needs_attention: bool,
    /// Small text badge (e.g. unread count). Reserved; currently unused.
    pub badge: Option<String>,
}

/// Full snapshot of what the dock should show. Emitted as `dock://state`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DockState {
    /// Monotonic counter; the UI may drop snapshots with a lower revision.
    /// Two snapshots of one revision may differ only in a reading no shown
    /// item uses (`power`, `volume`, `clockFormat`).
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub revision: u64,
    pub items: Vec<DockItem>,
    /// The OS foreground window, when it is one the engine tracks. It may be a
    /// window no item shows (e.g. on another virtual desktop); use
    /// `DockItem.isActive` / `WindowInfo.isActive` for highlighting.
    pub foreground: Option<WindowId>,
    /// The per-dock facts: one entry per dock window whose monitor
    /// the app layer has told the engine (`EngineInput::DockPanelChanged`),
    /// sorted by monitor id. A dock page finds its own entry by the monitor
    /// it sits on; a dock with no entry (yet) takes both facts as `false`.
    #[serde(default)]
    pub docks: Vec<DockFacts>,
    /// The power source and the battery: the last
    /// `PlatformEvent::PowerChanged`, whatever the settings say; every field
    /// `false` and no percent until the first one. The battery item's face
    /// is drawn from it. A new reading alone bumps the revision only while
    /// the battery item is shown, or when it brings or takes that item
    /// (`hasBattery` flipping with `items.battery` on).
    #[serde(default)]
    pub power: PowerStatus,
    /// The default output device's volume: the last
    /// `PlatformEvent::VolumeChanged`, whatever the settings say; `null`
    /// until the first one, and while there is no default output device or
    /// its volume cannot be read. The volume item's face and title are drawn
    /// from it. A new reading alone bumps the revision only while the volume
    /// item is shown.
    #[serde(default)]
    pub volume: Option<VolumeStatus>,
    /// The user's regional format for the clock: the last
    /// `PlatformEvent::ClockFormatChanged`, whatever the settings say; both
    /// fields `null` until the first one. The clock item's face, title pill
    /// and accessible name are formatted with it. A new reading alone bumps
    /// the revision only while the clock item is shown.
    #[serde(default)]
    pub clock_format: ClockFormat,
}

/// The power source and the system battery, part of [`DockState`].
/// The platform reads it (`Platform::power_status`, `GetSystemPowerStatus`
/// on Windows) and reports changes with `PlatformEvent::PowerChanged`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PowerStatus {
    /// The machine has a system battery. A desktop PC (and a laptop whose
    /// battery the system does not see) has none: the dock then shows no
    /// battery item.
    pub has_battery: bool,
    /// The battery's charge, 0 to 100; `null` when the system does not know
    /// it, and always without a battery.
    pub percent: Option<u8>,
    /// The battery is charging.
    pub charging: bool,
    /// The machine runs on AC power (plugged in), charging or not.
    pub on_ac: bool,
    /// Battery saver (energy saver) is on.
    pub saver: bool,
}

/// The default output device's master volume, part of
/// [`DockState`]. The platform reads it (`Platform::volume_status`; on
/// Windows the default console render endpoint's `IAudioEndpointVolume`)
/// and reports changes with `PlatformEvent::VolumeChanged`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct VolumeStatus {
    /// The master volume, 0 to 100: the endpoint's level (0.0 to 1.0, the
    /// scale Windows' own slider shows) × 100, rounded.
    pub percent: u8,
    /// The output is muted. The level is kept while muted, as Windows keeps
    /// it.
    pub muted: bool,
}

/// The user's regional format as Windows has it, for the clock,
/// part of [`DockState`]. The page's own locale is WebView2's language,
/// which follows the Windows display language rather than the regional
/// format, so the clock takes its locale and hour cycle from here. The
/// platform reads it (`Platform::clock_format`; `GetUserDefaultLocaleName`
/// and the short time format on Windows) and reports changes with
/// `PlatformEvent::ClockFormatChanged`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ClockFormat {
    /// The regional format's BCP 47 name, such as `en-GB`; `null` when the
    /// platform does not say.
    pub locale: Option<String>,
    /// `true` when the user's short time format is a 12-hour one (`h`),
    /// `false` for a 24-hour one (`H`); `null` when the platform does not
    /// say (or the format shows no hour).
    pub hour12: Option<bool>,
}

/// What one dock window needs to know about its own monitor, part
/// of [`DockState`]. The two facts are independent; the UI combines them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DockFacts {
    /// The monitor this dock window sits on (`dock.html?monitor=<id>`; the
    /// primary monitor for the `dock` window).
    pub monitor: MonitorId,
    /// The foreground window owns the whole of this monitor: a game, a
    /// browser after F11, a video in fullscreen, a slideshow. The
    /// platform decides (`PlatformEvent::FullscreenChanged`, with the
    /// window's monitor); the engine sets it on the dock of that monitor,
    /// or on the primary monitor's dock when the platform could not tell
    /// the monitor; the UI gets that dock out of the way while it is set,
    /// whatever `autoHide.mode` says.
    pub fullscreen: bool,
    /// A window of another process overlaps this dock's resting panel:
    /// visible, not minimised, not cloaked, with a frame that
    /// intersects the panel rect the dock's page reported through
    /// `set_dock_panel_rect` (`crate::overlap`). `false` while no panel
    /// rect is known for the dock (before its page's first report, or
    /// while its window is hidden). The UI hides the dock after
    /// `autoHide.delayMs` while it is set and `autoHide.mode` is
    /// `whenOverlapped`, and shows it again when it clears.
    pub overlapped: bool,
}
