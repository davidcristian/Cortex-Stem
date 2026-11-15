//! Events flowing from a platform adapter into the engine, and the engine's
//! input envelope.

use serde::{Deserialize, Serialize};

use crate::model::{
    AppId, AppIdentity, ClockFormat, MonitorId, MonitorInfo, PowerStatus, RawWindow, Rect,
    RecycleBinState, ThemeInfo, WindowId,
};
use crate::settings::Settings;

/// Something the OS told us. Produced by `stem_platform::Platform` on its own
/// thread and delivered through a channel; the engine consumes them in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PlatformEvent {
    /// Full resynchronisation. Sent once after `start()` and whenever the
    /// platform suspects it missed events (display change, resume from sleep).
    WindowsSnapshot(Vec<RawWindow>),
    WindowAdded(RawWindow),
    /// Title, state (minimised/cloaked/visible), monitor or icon changed.
    WindowUpdated(RawWindow),
    WindowRemoved(WindowId),
    ForegroundChanged(Option<WindowId>),
    #[serde(rename_all = "camelCase")]
    WindowAttention {
        id: WindowId,
        flashing: bool,
    },
    MonitorsChanged(Vec<MonitorInfo>),
    ThemeChanged(ThemeInfo),
    RecycleBinChanged(RecycleBinState),
    /// The foreground window started (`active`) or stopped owning the whole
    /// screen: the platform's verdict from the shell's notification
    /// state and the window's frame against its monitor, re-evaluated on
    /// foreground changes and on moves of the foreground window, and sent
    /// only when it changes (plus once after the start snapshot). `monitor`
    /// is the window's monitor while `active` (`None` when the
    /// platform could not tell, and always while inactive). The engine sets
    /// `DockFacts::fullscreen` on the dock of that monitor, or on the
    /// primary monitor's dock when the verdict names none.
    #[serde(rename_all = "camelCase")]
    FullscreenChanged {
        active: bool,
        monitor: Option<MonitorId>,
    },
    /// The power source, the battery's charge or battery saver changed:
    /// the platform's fresh reading, sent only when it differs from
    /// the last one sent (plus once after the start snapshot). The engine
    /// keeps it as `DockState::power`.
    PowerChanged(PowerStatus),
    /// The user's regional format changed: the platform's fresh
    /// reading of the locale and the short time format's hour cycle, sent
    /// only when it differs from the last one sent (plus once after the
    /// start snapshot). The engine keeps it as `DockState::clock_format`.
    ClockFormatChanged(ClockFormat),
}

/// Everything the engine can be fed.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineInput {
    Platform(PlatformEvent),
    SettingsChanged(Settings),
    /// Answer to an `EngineOutcome::unresolved_apps` request.
    IdentityResolved {
        app_id: AppId,
        identity: AppIdentity,
    },
    /// The request for `app_id` could not even be attempted (the app had no
    /// window and no pin by the time the resolver got to it). The engine
    /// forgets that it asked, so the next appearance of the app requests the
    /// identity again; no fallback identity is stored.
    IdentityUnavailable {
        app_id: AppId,
    },
    /// Where one dock window's panel rests on screen: the
    /// rect its page reported through `set_dock_panel_rect`, converted by
    /// the app layer to physical pixels of the virtual desktop, the same
    /// space as `RawWindow::rect`. `label` is the dock window's label
    /// (`dock`, `dock-<id>`), `monitor` the monitor the app layer placed
    /// it on (`None` while it could not tell: the dock then has no entry
    /// in `DockState::docks`), `rect` `None` while the window is hidden
    /// (the tray, the toggle shortcut) or before the page's first report,
    /// which clears the dock's `overlapped`. `monitor` and `rect` both
    /// `None` means the window is gone: the engine forgets the label.
    DockPanelChanged {
        label: String,
        monitor: Option<MonitorId>,
        rect: Option<Rect>,
    },
}

impl From<PlatformEvent> for EngineInput {
    fn from(ev: PlatformEvent) -> Self {
        Self::Platform(ev)
    }
}
