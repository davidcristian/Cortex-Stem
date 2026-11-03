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
//! - `recycle_bin` SHQueryRecycleBin, open, empty
//! - `appbar`      SHAppBarMessage space reservation (M4)
//! - `shell`       Start menu, Task View, Show Desktop
//! - `dock_window` extended styles for our own windows

pub mod actions;
pub mod appbar;
pub mod dock_window;
pub mod enumerate;
pub mod events;
pub mod icons;
pub mod identity;
pub mod launch;
pub mod monitors;
pub mod recycle_bin;
pub mod shell;
pub mod theme;
pub mod util;

use parking_lot::Mutex;
use stem_core::{
    AppIdentity, IconKey, LaunchTarget, MonitorInfo, RawWindow, RecycleBinState, ShellAction,
    ThemeInfo, WindowAction, WindowId,
};

use crate::{
    DockWindowOptions, EventSink, HitRegion, NativeWindowHandle, Platform, PlatformError,
    PlatformResult,
};

/// The real platform. Cheap to construct; `start` spins up the event thread.
#[derive(Debug, Default)]
pub struct WindowsPlatform {
    hook: Mutex<Option<events::EventHookThread>>,
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

    fn shell_action(&self, action: ShellAction) -> PlatformResult<()> {
        shell::perform(action)
    }

    fn prepare_dock_window(
        &self,
        handle: NativeWindowHandle,
        options: DockWindowOptions,
    ) -> PlatformResult<()> {
        dock_window::prepare(handle, options)
    }

    fn set_dock_hit_region(
        &self,
        handle: NativeWindowHandle,
        region: Option<HitRegion>,
    ) -> PlatformResult<()> {
        dock_window::set_hit_region(handle, region)
    }

    fn foreground_native_window(&self) -> Option<NativeWindowHandle> {
        dock_window::foreground_handle()
    }

    fn finish_popup(&self, dock: NativeWindowHandle, previous: Option<NativeWindowHandle>) {
        dock_window::finish_popup(dock, previous);
    }
}
