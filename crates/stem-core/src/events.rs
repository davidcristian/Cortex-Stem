//! Events flowing from a platform adapter into the engine, and the engine's
//! input envelope.

use serde::{Deserialize, Serialize};

use crate::model::{
    AppId, AppIdentity, MonitorInfo, RawWindow, RecycleBinState, ThemeInfo, WindowId,
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
}

impl From<PlatformEvent> for EngineInput {
    fn from(ev: PlatformEvent) -> Self {
        Self::Platform(ev)
    }
}
