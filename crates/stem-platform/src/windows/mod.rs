//! Windows implementation of [`Platform`].
//!
//! This directory is the **only** place in the workspace where `unsafe` and
//! the `windows` crate are allowed. Read
//! `docs/development/windows-rules.md` before editing.
//!
//! Modules:
//! - `util`        shared helpers (handles, strings, errors, COM)
//! - `enumerate`   which windows qualify, and their `RawWindow` facts
//! - `events`      WinEvent hook + message loop thread
//! - `actions`     activate / minimise / restore / close
//! - `monitors`    monitors, work areas, DPI
//! - `icons`       HICON/exe/UWP icons -> PNG, with cache
//! - `identity`    display names, AUMIDs, .lnk resolution
//! - `launch`      ShellExecute & friends
//! - `theme`       accent colour, light/dark, contrast
//! - `recycle_bin` SHQueryRecycleBin, open, empty; recycle files
//! - `appbar`      SHAppBarMessage space reservation (M4)
//! - `shell`       Start menu, Task View, Show Desktop
//! - `dock_window` extended styles + subclass of our own windows
//! - `hit_test`    cursor-tracking thread for the hit region
//! - `taskbar`     the stock taskbar's auto-hide state
//! - `fullscreen`  does the foreground window own the screen
//! - `dialog`      native message boxes (the crash report)
//! - `virtual_desktop` which virtual desktop a window is on

pub mod actions;
pub mod appbar;
pub mod dialog;
pub mod dock_window;
pub mod enumerate;
pub mod events;
pub mod fullscreen;
pub mod hit_test;
pub mod icons;
pub mod identity;
pub mod launch;
pub mod monitors;
pub mod recycle_bin;
pub mod shell;
pub mod taskbar;
pub mod theme;
pub mod util;
pub mod virtual_desktop;

use parking_lot::Mutex;
use stem_core::{
    AppIdentity, IconKey, LaunchTarget, MonitorInfo, RawWindow, RecycleBinState, ShellAction,
    ThemeInfo, WindowAction, WindowId,
};

use crate::{
    DockWindowOptions, EventSink, HitRegion, NativeWindowHandle, Platform, PlatformError,
    PlatformResult,
};

/// The real platform. Cheap to construct; `start` spins up the event thread;
/// the hit-test thread starts lazily on the first `set_dock_hit_region`.
#[derive(Debug, Default)]
pub struct WindowsPlatform {
    hook: Mutex<Option<events::EventHookThread>>,
    hit_test: Mutex<Option<hit_test::HitTestThread>>,
    icons: icons::IconCache,
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Platform for WindowsPlatform {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn start(&self, sink: EventSink) -> PlatformResult<()> {
        let mut guard = self.hook.lock();
        if guard.is_some() {
            return Err(PlatformError::Other("platform already started".into()));
        }
        *guard = Some(events::EventHookThread::start(sink)?);
        Ok(())
    }

    fn stop(&self) {
        if let Some(thread) = self.hook.lock().take() {
            thread.stop();
        }
        if let Some(tracker) = self.hit_test.lock().take() {
            tracker.stop();
        }
    }

    fn windows(&self) -> PlatformResult<Vec<RawWindow>> {
        enumerate::enumerate_taskbar_windows()
    }

    fn foreground_window(&self) -> PlatformResult<Option<WindowId>> {
        Ok(actions::foreground_window())
    }

    fn window_action(&self, id: WindowId, action: WindowAction) -> PlatformResult<()> {
        actions::perform(id, action)
    }

    fn identify_window(&self, id: WindowId) -> PlatformResult<AppIdentity> {
        let raw =
            enumerate::query_window(util::hwnd(id))?.ok_or(PlatformError::WindowNotFound(id))?;
        identity::identify_window(&raw)
    }

    fn identify_target(&self, target: &LaunchTarget) -> PlatformResult<AppIdentity> {
        identity::identify_target(target)
    }

    fn launch(&self, target: &LaunchTarget) -> PlatformResult<()> {
        launch::launch(target)
    }

    fn icon_png(&self, key: &IconKey, size: u32) -> PlatformResult<Option<Vec<u8>>> {
        if key.is_shell() {
            return Ok(None);
        }
        self.icons.get_or_load(key, size)
    }

    fn monitors(&self) -> PlatformResult<Vec<MonitorInfo>> {
        monitors::enumerate_monitors()
    }

    fn theme(&self) -> PlatformResult<ThemeInfo> {
        theme::current_theme()
    }

    fn recycle_bin(&self) -> PlatformResult<RecycleBinState> {
        recycle_bin::query()
    }

    fn delete_to_recycle_bin(&self, paths: &[String]) -> PlatformResult<()> {
        recycle_bin::recycle(paths)
    }

    fn shell_action(&self, action: ShellAction) -> PlatformResult<()> {
        shell::perform(action)
    }

    fn taskbar_auto_hide(&self) -> PlatformResult<bool> {
        taskbar::auto_hide()
    }

    fn set_taskbar_auto_hide(&self, enabled: bool) -> PlatformResult<()> {
        taskbar::set_auto_hide(enabled)
    }

    fn ask_yes_no(&self, title: &str, text: &str) -> PlatformResult<bool> {
        dialog::ask_yes_no(title, text)
    }

    fn prepare_dock_window(
        &self,
        handle: NativeWindowHandle,
        options: DockWindowOptions,
    ) -> PlatformResult<()> {
        dock_window::prepare(handle, options)
    }

    /// `Some`: hand the region to the tracker thread (started on the first
    /// call); `None`: stop tracking the window and make it clickable (what
    /// the app layer sends while `behavior.clickThrough` is off).
    fn set_dock_hit_region(
        &self,
        handle: NativeWindowHandle,
        region: Option<HitRegion>,
    ) -> PlatformResult<()> {
        let mut guard = self.hit_test.lock();
        match region {
            Some(region) => {
                if guard.is_none() {
                    *guard = Some(hit_test::HitTestThread::start()?);
                }
                if let Some(tracker) = guard.as_ref() {
                    tracker.track(handle, region);
                }
            }
            None => {
                if let Some(tracker) = guard.as_ref() {
                    tracker.untrack(handle);
                }
            }
        }
        Ok(())
    }

    fn foreground_native_window(&self) -> Option<NativeWindowHandle> {
        dock_window::foreground_handle()
    }

    fn finish_popup(&self, dock: NativeWindowHandle, previous: Option<NativeWindowHandle>) {
        dock_window::finish_popup(dock, previous);
    }
}
