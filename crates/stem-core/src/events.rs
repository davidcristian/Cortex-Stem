//! Events flowing from a platform adapter into the engine, and the engine's
//! input envelope.

use serde::{Deserialize, Serialize};

use crate::model::{
    AppId, AppIdentity, MonitorInfo, RawWindow, Rect, RecycleBinState, ThemeInfo, WindowId,
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
    /// The foreground window started (`true`) or stopped (`false`) owning
    /// the whole screen: the platform's verdict from the shell's
    /// notification state and the window's frame against its monitor,
    /// re-evaluated on foreground changes and on moves of the foreground
    /// window, and sent only when it changes (plus once after the start
    /// snapshot). Stored as `DockState::fullscreen`.
    FullscreenChanged(bool),
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
    /// Where the dock's panel rests on screen: the rect the UI
    /// reported through `set_dock_panel_rect`, converted by the app layer to
    /// physical pixels of the virtual desktop, the same space as
    /// `RawWindow::rect`. `None` while the dock window is hidden (the tray,
    /// the toggle shortcut) or before the UI's first report; it clears
    /// `DockState::overlapped`.
    DockPanelChanged(Option<Rect>),
}

impl From<PlatformEvent> for EngineInput {
    fn from(ev: PlatformEvent) -> Self {
        Self::Platform(ev)
    }
}
