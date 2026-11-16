//! The [`Platform`] trait and the sink its events go to.

use std::sync::Arc;
use std::time::Duration;

use stem_core::{
    AppIdentity, ClockFormat, IconKey, LaunchTarget, MonitorInfo, PlatformEvent, PowerStatus,
    RawWindow, RecycleBinState, ScrollDirection, ShellAction, ThemeInfo, VolumeStatus,
    WindowAction, WindowId,
};

use crate::{
    AppBarRequest, AppBarSink, DockBackdrop, DockWindowOptions, HitRegion, NativeWindowHandle,
    PlatformError, PlatformResult,
};

/// Where a platform delivers its events. Called from the platform's own
/// thread; implementations must be cheap and non-blocking (push to a channel).
pub type EventSink = Arc<dyn Fn(PlatformEvent) + Send + Sync + 'static>;

/// The interface to the OS. Every method is synchronous and may be called from any
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
    /// ([`rules::recycle`](crate::rules::recycle)); the others are skipped with a warning, and when
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

    /// The default output device's volume right now: `None` when
    /// there is no default output device or its volume cannot be read.
    /// Changes also arrive as `PlatformEvent::VolumeChanged` once the
    /// platform is started. On Windows the platform's volume thread reads
    /// it live (`IAudioEndpointVolume`) and this waits for its answer,
    /// bounded (about 250 ms, then [`PlatformError::Timeout`]); it fails
    /// before `start` and after `stop`. Never call it on the UI thread.
    /// Optional; the default reports it unsupported.
    fn volume_status(&self) -> PlatformResult<Option<VolumeStatus>> {
        Err(PlatformError::Unsupported("widget support"))
    }

    /// Set the default output device's master volume to `percent` (0 to
    /// 100; more is taken as 100), leaving the mute as it is. The
    /// change is reported as `PlatformEvent::VolumeChanged` like any other.
    /// Fails without an output device, before `start` and after `stop`;
    /// bounded like [`Platform::volume_status`], whose answer reflects the
    /// new level once this returns. Never call it on the UI thread.
    /// Optional; the default reports it unsupported.
    fn set_volume(&self, _percent: u8) -> PlatformResult<()> {
        Err(PlatformError::Unsupported("widget support"))
    }

    /// Mute (`true`) or unmute the default output device, leaving its level
    /// as it is; otherwise as [`Platform::set_volume`]. Optional;
    /// the default reports it unsupported.
    fn set_volume_muted(&self, _muted: bool) -> PlatformResult<()> {
        Err(PlatformError::Unsupported("widget support"))
    }

    /// One wheel notch over the volume item: read the volume now,
    /// apply `stem_core::engine::volume_step` and set the level, then the
    /// mute, where they change, as one operation that no other volume
    /// request interleaves with. Returns the volume it set, or `None` when
    /// there was nothing to do (no output device, the end of the range). On
    /// Windows it is one request to the volume thread, bounded like
    /// [`Platform::volume_status`]; a request the thread takes only after
    /// its caller timed out is dropped, not done late. Never call it on the
    /// UI thread or on an async runtime's worker. Optional; the default
    /// reports it unsupported.
    fn step_volume(&self, _direction: ScrollDirection) -> PlatformResult<Option<VolumeStatus>> {
        Err(PlatformError::Unsupported("widget support"))
    }

    /// Whether Windows' own *Automatically hide the taskbar* is on right now
    /// (`SHAppBarMessage(ABM_GETSTATE)`). A message to the tray
    /// window that waits on Explorer: never call it on the UI thread. Fails
    /// when there is no tray, and with [`PlatformError::Timeout`] when
    /// Explorer does not answer within about 500 ms (the Windows
    /// implementation waits on a helper thread). The decision of what to do
    /// with the answer is [`rules::taskbar`](crate::rules::taskbar). Optional; the default reports
    /// it unsupported.
    fn taskbar_auto_hide(&self) -> PlatformResult<bool> {
        Err(PlatformError::Unsupported("taskbar auto-hide"))
    }

    /// Turn Windows' own *Automatically hide the taskbar* on or off
    /// (`SHAppBarMessage(ABM_SETSTATE)`). Windows persists the new
    /// state itself (the Taskbar settings page shows it and it survives an
    /// Explorer restart), which is why the app layer remembers the previous
    /// one ([`rules::taskbar`](crate::rules::taskbar)). Idempotent. A message to the tray window
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
    /// `SHAppBarMessage`, waits
    /// [`rules::appbar::Settle::DELAY_MS`](crate::rules::appbar::Settle::DELAY_MS) for a
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
    /// `rules::appbar::Bars`. `handle` must be a window [`Platform::prepare_dock_window`] prepared:
    /// the shell's notifications arrive through its subclass. Optional; the default
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
