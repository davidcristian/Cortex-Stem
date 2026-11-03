//! The event thread.
//!
//! Contract:
//! - [`EventHookThread::start`] spawns a thread that: creates a message-only
//!   window, installs `SetWinEventHook` out-of-context hooks for
//!   `EVENT_OBJECT_CREATE/DESTROY/SHOW/HIDE/CLOAKED/UNCLOAKED/NAMECHANGE`,
//!   `EVENT_SYSTEM_FOREGROUND`, `EVENT_SYSTEM_MINIMIZESTART/END`, registers a
//!   shell hook window (`RegisterShellHookWindow`) for `HSHELL_FLASH`, and then
//!   runs a `GetMessageW` loop.
//! - Hook callbacks only translate the event and push a [`stem_core::PlatformEvent`]
//!   into `sink`; all filtering goes through `enumerate::query_window`.
//! - The thread emits an initial `WindowsSnapshot`, `MonitorsChanged`,
//!   `ThemeChanged`, `RecycleBinChanged`, `ForegroundChanged` after setup.
//! - `WM_DISPLAYCHANGE`/`WM_DPICHANGED` -> `MonitorsChanged` + `WindowsSnapshot`.
//! - `WM_SETTINGCHANGE` ("ImmersiveColorSet") and `WM_DWMCOLORIZATIONCOLORCHANGED`
//!   -> `ThemeChanged` (via `theme::current_theme`).
//! - A timer polls `recycle_bin::query` every few seconds and emits
//!   `RecycleBinChanged` on change.
//! - [`EventHookThread::stop`] posts `WM_QUIT`, unhooks, and joins (bounded).

use crate::{EventSink, PlatformError, PlatformResult};

/// Handle to the running event thread. Dropping it without `stop` detaches
/// the thread (it keeps running until process exit); prefer `stop`.
#[derive(Debug)]
pub struct EventHookThread {
    _private: (),
}

impl EventHookThread {
    pub fn start(_sink: EventSink) -> PlatformResult<Self> {
        Err(PlatformError::Unsupported("window event tracking"))
    }

    pub fn stop(self) {}
}
