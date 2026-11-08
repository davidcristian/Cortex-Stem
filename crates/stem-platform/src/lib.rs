//! `stem-platform` — everything that talks to the operating system.
//!
//! The [`Platform`] trait is the seam between the pure engine (`stem-core`)
//! and the OS. Two implementations exist:
//! - [`mock::MockPlatform`]: scripted, deterministic, runs on every host. Used
//!   by tests and by the app when `STEM_PLATFORM=mock`.
//! - [`windows::WindowsPlatform`] (`cfg(windows)` only): the real thing.
//!
//! Boundary rules (`AGENTS.md` §4, `docs/development/windows-rules.md`):
//! - no OS types in this file or in the trait;
//! - `unsafe` only under `src/windows/`;
//! - blocking or message-loop work happens on threads owned by this crate.

#![cfg_attr(not(windows), forbid(unsafe_code))]

use std::sync::Arc;

use stem_core::{
    AppIdentity, IconKey, LaunchTarget, MonitorInfo, PlatformEvent, RawWindow, Rect,
    RecycleBinState, ShellAction, ThemeInfo, WindowAction, WindowId,
};

pub mod mock;
pub mod rules;
#[cfg(windows)]
pub mod windows;

/// Where a platform delivers its events. Called from the platform's own
/// thread; implementations must be cheap and non-blocking (push to a channel).
pub type EventSink = Arc<dyn Fn(PlatformEvent) + Send + Sync + 'static>;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// The feature is not implemented in this build. The payload names the
    /// missing feature (e.g. `"icon extraction"`).
    #[error("{0} is not implemented on this platform")]
    Unsupported(&'static str),
    #[error("window {0:?} no longer exists")]
    WindowNotFound(WindowId),
    #[error("access denied (elevated or protected process)")]
    AccessDenied,
    #[error("operation timed out")]
    Timeout,
    #[error("OS error {code:#010x}: {message}")]
    Os { code: u32, message: String },
    #[error("{0}")]
    Other(String),
}

pub type PlatformResult<T> = Result<T, PlatformError>;

/// Native handle of a window the *app layer* created (a Tauri dock window).
/// On Windows this is the `HWND` value. Opaque everywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeWindowHandle(pub isize);

/// How a dock window should behave at the OS level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockWindowOptions {
    /// Clicking the dock must not take keyboard focus from the active app.
    pub no_activate: bool,
    /// Hide from Alt+Tab and the stock taskbar.
    pub tool_window: bool,
    /// Ask the compositor not to round or shadow the window; we draw our own.
    pub no_system_decorations: bool,
}

impl Default for DockWindowOptions {
    fn default() -> Self {
        Self {
            no_activate: true,
            tool_window: true,
            no_system_decorations: true,
        }
    }
}

/// Where a dock window accepts mouse input. Both rectangles are in
/// **physical pixels, relative to the window's client area**.
///
/// The window becomes clickable when the cursor enters `panel` (the visible
/// dock panel) and click-through again only once the cursor has left
/// `envelope` (the panel plus everything magnification can lift above it),
/// so an icon that grew past the panel's edge stays clickable. `envelope`
/// contains `panel`; the implementation may treat a smaller envelope as
/// equal to the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitRegion {
    pub panel: Rect,
    pub envelope: Rect,
}

/// The OS seam. Every method is synchronous and may be called from any
/// thread; implementations must be internally thread-safe and must not block
/// for long (bounded timeouts, see `docs/development/windows-rules.md`).
pub trait Platform: Send + Sync + 'static {
    /// Human-readable implementation name (`"windows"`, `"mock"`).
    fn name(&self) -> &'static str;

    /// Begin delivering events to `sink`. Must emit a
    /// [`PlatformEvent::WindowsSnapshot`], [`PlatformEvent::MonitorsChanged`],
    /// [`PlatformEvent::ThemeChanged`] and [`PlatformEvent::RecycleBinChanged`]
    /// shortly after starting. Calling `start` twice is an error.
    fn start(&self, sink: EventSink) -> PlatformResult<()>;

    /// Stop delivering events and release OS resources. Idempotent.
    fn stop(&self);

    /// All windows that currently qualify for the dock.
    fn windows(&self) -> PlatformResult<Vec<RawWindow>>;

    fn foreground_window(&self) -> PlatformResult<Option<WindowId>>;

    fn window_action(&self, id: WindowId, action: WindowAction) -> PlatformResult<()>;

    /// Resolve display name, icon and launch target for a running window's app.
    fn identify_window(&self, id: WindowId) -> PlatformResult<AppIdentity>;

    /// Resolve display name, icon and canonical [`stem_core::AppId`] for
    /// something the user wants to pin (a path, a shortcut, a Store app).
    fn identify_target(&self, target: &LaunchTarget) -> PlatformResult<AppIdentity>;

    fn launch(&self, target: &LaunchTarget) -> PlatformResult<()>;

    /// PNG bytes for an icon key at roughly `size` px, or `None` when the key
    /// is unknown. `shell:*` keys are rendered by the frontend and return `None`.
    fn icon_png(&self, key: &IconKey, size: u32) -> PlatformResult<Option<Vec<u8>>>;

    fn monitors(&self) -> PlatformResult<Vec<MonitorInfo>>;

    fn theme(&self) -> PlatformResult<ThemeInfo>;

    fn recycle_bin(&self) -> PlatformResult<RecycleBinState>;

    /// Move files and folders to the Recycle Bin (what a file drop on
    /// its item does). `paths` are the dropped paths as the shell gave them.
    ///
    /// Each one must be a plain absolute file path that exists
    /// ([`rules::recycle`]); the others are skipped with a warning, and when
    /// nothing is left the call fails. The rest go to the bin in one shell
    /// operation, with the shell's own confirmation, progress and error UI:
    /// an item that cannot be recycled (its drive has no bin, it is too
    /// large) is only deleted for good once the user agreed to the shell's
    /// warning. The bin's new state is not reported here; the caller asks
    /// [`Platform::recycle_bin`] when it wants it sooner than the poll.
    ///
    /// **Blocks** until the shell is done, which includes the user
    /// answering its dialogs: never call it on the UI thread or on a thread
    /// that must stay responsive. Fails when the user cancelled or an item
    /// was refused (in use, access denied); what went to the bin before
    /// that stays there.
    fn delete_to_recycle_bin(&self, paths: &[String]) -> PlatformResult<()>;

    fn shell_action(&self, action: ShellAction) -> PlatformResult<()>;

    /// Whether Windows' own *Automatically hide the taskbar* is on right now
    /// (`SHAppBarMessage(ABM_GETSTATE)`). A message to the tray
    /// window that waits on Explorer: never call it on the UI thread. The
    /// decision of what to do with the answer is [`rules::taskbar`].
    /// Optional; the default reports it unsupported.
    fn taskbar_auto_hide(&self) -> PlatformResult<bool> {
        Err(PlatformError::Unsupported("taskbar auto-hide"))
    }

    /// Turn Windows' own *Automatically hide the taskbar* on or off
    /// (`SHAppBarMessage(ABM_SETSTATE)`). Windows persists the new
    /// state itself (the Taskbar settings page shows it and it survives an
    /// Explorer restart), which is why the app layer remembers the previous
    /// one ([`rules::taskbar`]). Idempotent. A message to the tray window
    /// that waits on Explorer: never call it on the UI thread. Fails when
    /// the taskbar did not take the state (no tray to talk to). Optional;
    /// the default reports it unsupported.
    fn set_taskbar_auto_hide(&self, _enabled: bool) -> PlatformResult<()> {
        Err(PlatformError::Unsupported("taskbar auto-hide"))
    }

    /// Apply OS-level styles to a dock window the app layer created.
    ///
    /// Must be called **once per window, on the thread that created the
    /// window** (the Tauri main thread, inside `setup`): the Windows
    /// implementation subclasses the window, which the OS only allows from
    /// the window's own thread, and the subclass stays installed for the
    /// window's lifetime.
    fn prepare_dock_window(
        &self,
        handle: NativeWindowHandle,
        options: DockWindowOptions,
    ) -> PlatformResult<()>;

    /// Restrict mouse hit-testing of a dock window to `region` (see
    /// [`HitRegion`]); clicks outside fall through to whatever is beneath.
    /// `None` restores normal hit-testing (the whole window is clickable).
    /// Must return quickly: the tracking itself happens on a thread owned by
    /// the platform. Optional; the default reports it unsupported.
    fn set_dock_hit_region(
        &self,
        _handle: NativeWindowHandle,
        _region: Option<HitRegion>,
    ) -> PlatformResult<()> {
        Err(PlatformError::Unsupported("click-through"))
    }

    /// The native window that currently has the foreground, if any. Captured
    /// before a native popup menu is shown on a `no_activate` dock window so
    /// that [`Platform::finish_popup`] can hand the foreground back.
    fn foreground_native_window(&self) -> Option<NativeWindowHandle> {
        None
    }

    /// Complete the native popup-menu protocol right after the menu closed:
    /// whatever the OS needs so the next outside click is delivered normally,
    /// then give the foreground back to `previous` (the dock must never stay
    /// focused). A no-op where there is nothing to do.
    fn finish_popup(&self, _dock: NativeWindowHandle, _previous: Option<NativeWindowHandle>) {}
}

/// Whether `platform` is the scripted [`mock::MockPlatform`] (by its
/// [`Platform::name`]), whichever way it was chosen: `STEM_PLATFORM=mock`, or
/// a host that has no real implementation. The app layer uses this to keep
/// demo runs away from the real settings file.
pub fn is_mock(platform: &dyn Platform) -> bool {
    platform.name() == MOCK_PLATFORM_NAME
}

/// What [`mock::MockPlatform::name`] returns.
const MOCK_PLATFORM_NAME: &str = "mock";

/// Build the platform for this host. Honors `STEM_PLATFORM=mock` on any host.
pub fn create_platform() -> Arc<dyn Platform> {
    if std::env::var("STEM_PLATFORM").is_ok_and(|v| v.eq_ignore_ascii_case("mock")) {
        return Arc::new(mock::MockPlatform::with_demo_scenario());
    }
    #[cfg(windows)]
    {
        Arc::new(windows::WindowsPlatform::new())
    }
    #[cfg(not(windows))]
    {
        Arc::new(mock::MockPlatform::with_demo_scenario())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_mock_recognises_the_mock_platform_by_name() {
        let mock: Arc<dyn Platform> = Arc::new(mock::MockPlatform::new());
        assert!(is_mock(mock.as_ref()));
        assert_eq!(mock.name(), MOCK_PLATFORM_NAME);

        struct Real;
        impl Platform for Real {
            fn name(&self) -> &'static str {
                "windows"
            }
            fn start(&self, _sink: EventSink) -> PlatformResult<()> {
                Ok(())
            }
            fn stop(&self) {}
            fn windows(&self) -> PlatformResult<Vec<RawWindow>> {
                Ok(vec![])
            }
            fn foreground_window(&self) -> PlatformResult<Option<WindowId>> {
                Ok(None)
            }
            fn window_action(&self, _id: WindowId, _action: WindowAction) -> PlatformResult<()> {
                Ok(())
            }
            fn identify_window(&self, id: WindowId) -> PlatformResult<AppIdentity> {
                Err(PlatformError::WindowNotFound(id))
            }
            fn identify_target(&self, _target: &LaunchTarget) -> PlatformResult<AppIdentity> {
                Err(PlatformError::Unsupported("test"))
            }
            fn launch(&self, _target: &LaunchTarget) -> PlatformResult<()> {
                Ok(())
            }
            fn icon_png(&self, _key: &IconKey, _size: u32) -> PlatformResult<Option<Vec<u8>>> {
                Ok(None)
            }
            fn monitors(&self) -> PlatformResult<Vec<MonitorInfo>> {
                Ok(vec![])
            }
            fn theme(&self) -> PlatformResult<ThemeInfo> {
                Ok(ThemeInfo::default())
            }
            fn recycle_bin(&self) -> PlatformResult<RecycleBinState> {
                Ok(RecycleBinState::default())
            }
            fn delete_to_recycle_bin(&self, _paths: &[String]) -> PlatformResult<()> {
                Ok(())
            }
            fn shell_action(&self, _action: ShellAction) -> PlatformResult<()> {
                Ok(())
            }
            fn prepare_dock_window(
                &self,
                _handle: NativeWindowHandle,
                _options: DockWindowOptions,
            ) -> PlatformResult<()> {
                Ok(())
            }
        }
        assert!(!is_mock(&Real));
    }
}
