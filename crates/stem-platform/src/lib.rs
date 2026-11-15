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
use std::time::Duration;

use stem_core::settings::DockPosition;
use stem_core::{
    AppIdentity, ClockFormat, IconKey, LaunchTarget, MonitorInfo, PlatformEvent, PowerStatus,
    RawWindow, Rect, RecycleBinState, ShellAction, ThemeInfo, WindowAction, WindowId,
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

/// The screen space a dock window reserves as an AppBar: a
/// band at its screen edge that the shell takes out of the monitor's work
/// area, so maximised windows stop short of the dock. Physical pixels of
/// the virtual desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppBarRequest {
    /// The full bounds of the window's monitor ([`MonitorInfo::bounds`]),
    /// never its work area: the work area shrinks once the band is
    /// reserved, so a band asked for on its edge would move inwards with
    /// every answer. The shell itself moves the band past the stock taskbar
    /// and other bars on that edge ([`rules::appbar`]).
    pub monitor: Rect,
    /// The edge the band lies on.
    pub edge: DockPosition,
    /// Across the edge: the panel's resting thickness plus its margin to
    /// the edge, not the magnification headroom. The platform keeps it
    /// between 1 px and half the monitor ([`rules::appbar::band_thickness`]).
    pub thickness: i32,
}

/// The shell's answer to an [`AppBarRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppBarGrant {
    /// The request this answers.
    pub request: AppBarRequest,
    /// The band the shell reserved, physical px: on the request's edge of
    /// its monitor, moved inwards past the bars already on that edge (the
    /// stock taskbar when it is visible there) and possibly shortened by
    /// bars on the neighbouring edges. The dock window goes with its edge
    /// side on this band's edge side ([`rules::appbar::window_rect`]).
    pub rect: Rect,
}

/// Where a platform reports what became of a dock window's AppBar:
/// - `Some` with the band the shell grants after **every** request the
///   platform applied, even when it is the band reported before (the
///   caller may have dropped it while a newer request was on its way), and
///   whenever the shell moves the band (a bar or the taskbar changing on
///   that edge, an Explorer restart), then only when it differs from the
///   last one reported;
/// - `None` once the window holds no band for a request it still makes (a
///   registration that failed, or a band the shell answered badly and that
///   was given up). The platform keeps asking for that request, after a
///   growing delay (`rules::appbar::Retry`), and reports `Some` once it
///   holds the band; the caller need not ask again. Nothing is reported
///   after a release the caller asked for, nor at
///   [`Platform::release_app_bars`].
///
/// Called on a thread of the platform's own, or (the mock) on the caller's:
/// it must be cheap and must not block (post to the UI thread), and must
/// not call back into [`Platform::set_dock_app_bar`] synchronously.
pub type AppBarSink = Arc<dyn Fn(NativeWindowHandle, Option<AppBarGrant>) + Send + Sync + 'static>;

/// The system material of a backdrop window (a prototype,
/// `docs/research/backdrop/backdrop-window.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackdropMaterial {
    /// Desktop Acrylic: blurs whatever is behind the window.
    Acrylic,
    /// Mica: tinted by the desktop wallpaper.
    Mica,
}

/// What the native backdrop window under a dock window shows (a
/// prototype). Platform-local on purpose: the prototype changes neither the
/// IPC contract nor `stem-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockBackdrop {
    /// The dock's panel at rest, in **physical pixels of the virtual
    /// desktop**, rounded inwards: the material must never stick out of the
    /// panel.
    pub rect: Rect,
    /// The panel's corner radius in physical pixels.
    pub corner_radius: u32,
    pub material: BackdropMaterial,
    /// The dark variant of the material (the dock's effective theme).
    pub dark: bool,
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

    /// The power source, the battery and battery saver right now
    /// (`GetSystemPowerStatus` on Windows, which does not block). Changes
    /// also arrive as `PlatformEvent::PowerChanged` once the platform is
    /// started, so callers need this only for a reading of their own.
    /// Optional; the default reports it unsupported.
    fn power_status(&self) -> PlatformResult<PowerStatus> {
        Err(PlatformError::Unsupported("widget support"))
    }

    /// The user's regional format for the clock right now (on
    /// Windows `GetUserDefaultLocaleName` and the short time format's hour
    /// letter, which do not block). Changes also arrive as
    /// `PlatformEvent::ClockFormatChanged` once the platform is started, so
    /// callers need this only for a reading of their own. Optional; the
    /// default reports it unsupported.
    fn clock_format(&self) -> PlatformResult<ClockFormat> {
        Err(PlatformError::Unsupported("widget support"))
    }

    /// Whether Windows' own *Automatically hide the taskbar* is on right now
    /// (`SHAppBarMessage(ABM_GETSTATE)`). A message to the tray
    /// window that waits on Explorer: never call it on the UI thread. Fails
    /// when there is no tray, and with [`PlatformError::Timeout`] when
    /// Explorer does not answer within about 500 ms (the Windows
    /// implementation waits on a helper thread). The decision of what to do
    /// with the answer is [`rules::taskbar`]. Optional; the default reports
    /// it unsupported.
    fn taskbar_auto_hide(&self) -> PlatformResult<bool> {
        Err(PlatformError::Unsupported("taskbar auto-hide"))
    }

    /// Turn Windows' own *Automatically hide the taskbar* on or off
    /// (`SHAppBarMessage(ABM_SETSTATE)`). Windows persists the new
    /// state itself (the Taskbar settings page shows it and it survives an
    /// Explorer restart), which is why the app layer remembers the previous
    /// one ([`rules::taskbar`]). Idempotent. A message to the tray window
    /// that waits on Explorer: never call it on the UI thread. Fails when
    /// the taskbar did not take the state or there is no tray, and with
    /// [`PlatformError::Timeout`] when Explorer does not answer within
    /// about 500 ms. Optional; the default reports it unsupported.
    fn set_taskbar_auto_hide(&self, _enabled: bool) -> PlatformResult<()> {
        Err(PlatformError::Unsupported("taskbar auto-hide"))
    }

    /// Reserve screen space for a dock window as an AppBar (`Some`), or
    /// give it back (`None`). Returns at once, on any thread
    /// (the main thread included): the Windows implementation queues the
    /// request for a thread of its own (`stem-appbar`), which sends every
    /// `SHAppBarMessage`, waits [`rules::appbar::Settle::DELAY_MS`] for a
    /// newer request of the same window before it takes a band (a release
    /// does not wait), registers the window (`ABM_NEW`), proposes the band
    /// on the request's edge of the monitor's bounds (`ABM_QUERYPOS`), takes
    /// what the shell allows (`ABM_SETPOS`) and reports the band through
    /// `sink` ([`AppBarSink`]); a band it could not take is asked for again
    /// later. It answers the shell's notifications on that thread too (the
    /// dock window's subclass forwards them): a band the shell moves is
    /// asked for again and reported, and after an Explorer restart every
    /// bar registers again. Another edge or another monitor gives the old
    /// band back before the new one is taken. The bookkeeping is
    /// `rules::appbar::Bars`. `handle` must
    /// be a window [`Platform::prepare_dock_window`] prepared: the shell's
    /// notifications arrive through its subclass. Optional; the default
    /// reports it unsupported.
    fn set_dock_app_bar(
        &self,
        _handle: NativeWindowHandle,
        _request: Option<AppBarRequest>,
        _sink: AppBarSink,
    ) -> PlatformResult<()> {
        Err(PlatformError::Unsupported("appbar registration"))
    }

    /// Give back every band [`Platform::set_dock_app_bar`] reserved and
    /// wait at most `timeout` for the shell to take them (the app's quit).
    /// May be called on the thread that owns the dock windows, which is
    /// where the app quits: the wait keeps handling the messages other
    /// threads send to that thread's windows (the shell notifies the
    /// remaining bars while it removes one), so it cannot deadlock with
    /// the shell. Fails with [`PlatformError::Timeout`] when the shell did
    /// not finish in time (a hung Explorer); the release then still
    /// happens if the shell answers before the process ends. Nothing is
    /// reported to the sinks. Optional; the default reports it unsupported.
    fn release_app_bars(&self, _timeout: Duration) -> PlatformResult<()> {
        Err(PlatformError::Unsupported("appbar registration"))
    }

    /// Ask the user a yes/no question in a native, topmost message box
    /// (the next start after a crash asks whether to open the log
    /// folder). `Ok(true)` for *Yes*, `Ok(false)` for *No* or a box closed
    /// without an answer. The box has no owner window: the dock is
    /// `no_activate` and must not own an activating dialog.
    ///
    /// **Blocks** until the user answers: never call it on the UI thread or
    /// on a thread that must stay responsive (the app layer uses Tauri's
    /// blocking pool). Optional; the default reports it unsupported.
    fn ask_yes_no(&self, _title: &str, _text: &str) -> PlatformResult<bool> {
        Err(PlatformError::Unsupported("message box"))
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

    /// Show a native backdrop window with `backdrop`'s material, shape and
    /// place directly under the dock window `handle` in the z-order, or
    /// hide it (`None`): the backdrop prototype. A later call replaces the
    /// earlier one; an unchanged one costs nothing. Must return quickly and
    /// never block: the window lives on a thread owned by the platform. A
    /// host whose compositor has no such material logs that once and does
    /// nothing. Optional; the default reports it unsupported.
    fn set_dock_backdrop(
        &self,
        _handle: NativeWindowHandle,
        _backdrop: Option<DockBackdrop>,
    ) -> PlatformResult<()> {
        Err(PlatformError::Unsupported("dock backdrop"))
    }

    /// Give the keyboard focus to the dock window `handle` (the focus
    /// shortcut): make it the foreground window, which a `no_activate`
    /// window never becomes by a click, through the activation path of
    /// [`Platform::window_action`]'s `Activate` (the foreground fallbacks
    /// included). The window keeps its `no_activate` behaviour: a click on
    /// it still activates nothing, and once another window takes the
    /// foreground it is an ordinary dock again. The press of a global
    /// shortcut gives the process the right to take the foreground; called
    /// at any other moment the OS may refuse. `handle` must be a window
    /// [`Platform::prepare_dock_window`] prepared, visible. Fails with
    /// [`PlatformError::WindowNotFound`] for a window that is gone, and
    /// when every strategy was refused. Bounded but not instant (the
    /// fallbacks touch other threads' input state): never call it on the
    /// UI thread. Optional; the default reports it unsupported.
    fn focus_dock_window(&self, _handle: NativeWindowHandle) -> PlatformResult<()> {
        Err(PlatformError::Unsupported("keyboard focus"))
    }

    /// The native window that currently has the foreground, if any. Captured
    /// before a native popup menu is shown on a `no_activate` dock window so
    /// that [`Platform::finish_popup`] can hand the foreground back; also
    /// what tells the app layer whether a dock window has the keyboard
    /// focus.
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

/// Build the platform for this host. Honors `STEM_PLATFORM=mock` on any host
/// (surrounding blanks ignored: cmd's `set STEM_PLATFORM=mock && …` keeps
/// the space before `&&`).
pub fn create_platform() -> Arc<dyn Platform> {
    if std::env::var("STEM_PLATFORM").is_ok_and(|v| v.trim().eq_ignore_ascii_case("mock")) {
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
        // A platform that does not implement the focus shortcut says so,
        // naming the feature, instead of pretending it worked.
        assert!(matches!(
            Real.focus_dock_window(NativeWindowHandle(0x1234)),
            Err(PlatformError::Unsupported("keyboard focus"))
        ));
        // The same for the power reading.
        assert!(matches!(
            Real.power_status(),
            Err(PlatformError::Unsupported("widget support"))
        ));
        // And for the regional format.
        assert!(matches!(
            Real.clock_format(),
            Err(PlatformError::Unsupported("widget support"))
        ));
    }
}
