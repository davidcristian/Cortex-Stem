//! [`WindowsPlatform`]: the [`Platform`] implementation. Each method calls
//! the sibling module of `windows` that does the work.

use std::time::Duration;

use parking_lot::Mutex;
use stem_core::{
    AppIdentity, ClockFormat, IconKey, LaunchTarget, MonitorInfo, PowerStatus, RawWindow,
    RecycleBinState, ScrollDirection, ShellAction, ThemeInfo, VolumeStatus, WindowAction, WindowId,
};

use crate::{
    AppBarRequest, AppBarSink, DockBackdrop, DockWindowOptions, EventSink, HitRegion,
    NativeWindowHandle, Platform, PlatformError, PlatformResult,
};

use super::{
    actions, appbar, backdrop, dialog, dock_window, enumerate, events, hit_test, icons, identity,
    launch, locale, monitors, power, recycle_bin, shell, taskbar, theme, util, volume,
};

/// The real platform. Cheap to construct; `start` spins up the event thread
/// and the volume thread; the hit-test thread starts lazily on the first
/// `set_dock_hit_region`, the
/// AppBar thread on the first `set_dock_app_bar` that asks for a band, the
/// backdrop thread on the first `set_dock_backdrop` with a backdrop.
#[derive(Debug, Default)]
pub struct WindowsPlatform {
    hook: Mutex<Option<events::EventHookThread>>,
    /// The volume thread, started with the events thread.
    volume: Mutex<Option<volume::VolumeThread>>,
    hit_test: Mutex<Option<hit_test::HitTestThread>>,
    app_bars: Mutex<Option<appbar::AppBarThread>>,
    backdrop: Mutex<Option<backdrop::BackdropThread>>,
    icons: icons::IconCache,
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self::default()
    }

    /// The running volume thread's handle, taken out of the lock.
    fn volume_handle(&self) -> PlatformResult<volume::VolumeHandle> {
        self.volume
            .lock()
            .as_ref()
            .map(volume::VolumeThread::handle)
            .ok_or_else(|| PlatformError::Other("the volume thread is not running".into()))
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
        *guard = Some(events::EventHookThread::start(sink.clone())?);
        // The dock works without it: the volume reads as unavailable.
        match volume::VolumeThread::start(sink) {
            Ok(thread) => *self.volume.lock() = Some(thread),
            Err(e) => log::warn!("volume thread unavailable: {e}"),
        }
        Ok(())
    }

    fn stop(&self) {
        if let Some(thread) = self.hook.lock().take() {
            thread.stop();
        }
        // Out of the lock first: the stop waits (bounded) for the thread.
        let volume = self.volume.lock().take();
        if let Some(volume) = volume {
            volume.stop();
        }
        if let Some(tracker) = self.hit_test.lock().take() {
            tracker.stop();
        }
        // Out of the lock first: the stop waits (bounded) for the shell.
        let app_bars = self.app_bars.lock().take();
        if let Some(app_bars) = app_bars {
            app_bars.stop();
        }
        if let Some(backdrops) = self.backdrop.lock().take() {
            backdrops.stop();
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

    fn power_status(&self) -> PlatformResult<PowerStatus> {
        power::current()
    }

    fn clock_format(&self) -> PlatformResult<ClockFormat> {
        Ok(locale::current())
    }

    /// A live read on the volume thread; it waits outside the platform's
    /// lock, at most `volume::REQUEST_TIMEOUT`.
    fn volume_status(&self) -> PlatformResult<Option<VolumeStatus>> {
        self.volume_handle()?.read()
    }

    fn set_volume(&self, percent: u8) -> PlatformResult<()> {
        self.volume_handle()?.set_level(percent)
    }

    fn set_volume_muted(&self, muted: bool) -> PlatformResult<()> {
        self.volume_handle()?.set_muted(muted)
    }

    /// One message to the volume thread, which reads, steps and sets there.
    fn step_volume(&self, direction: ScrollDirection) -> PlatformResult<Option<VolumeStatus>> {
        self.volume_handle()?.step(direction)
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

    /// Queues the request for the AppBar thread, started by the first
    /// request for a band; a release before that has nothing to give back.
    fn set_dock_app_bar(
        &self,
        handle: NativeWindowHandle,
        request: Option<AppBarRequest>,
        sink: AppBarSink,
    ) -> PlatformResult<()> {
        let mut guard = self.app_bars.lock();
        if guard.is_none() {
            if request.is_none() {
                return Ok(());
            }
            *guard = Some(appbar::AppBarThread::start()?);
        }
        if let Some(app_bars) = guard.as_ref() {
            app_bars.set(handle, request, sink);
        }
        Ok(())
    }

    /// Waits outside the platform's lock, so nothing that needs the lock
    /// can be held up behind the shell.
    fn release_app_bars(&self, timeout: Duration) -> PlatformResult<()> {
        let queue = self
            .app_bars
            .lock()
            .as_ref()
            .map(appbar::AppBarThread::queue);
        match queue {
            Some(queue) => appbar::release_all(&queue, timeout),
            None => Ok(()),
        }
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

    /// `Some`: hand the request to the backdrop thread (spawned on the
    /// first one, without waiting for it); `None`: hide the window, if the
    /// thread has one. A build without `DWMWA_SYSTEMBACKDROP_TYPE` logs that
    /// once and does nothing (the backdrop is a prototype).
    fn set_dock_backdrop(
        &self,
        handle: NativeWindowHandle,
        backdrop: Option<DockBackdrop>,
    ) -> PlatformResult<()> {
        let mut guard = self.backdrop.lock();
        match backdrop {
            Some(wanted) => {
                if !backdrop::supported() {
                    return Ok(());
                }
                if guard.is_none() {
                    *guard = Some(backdrop::BackdropThread::start()?);
                }
                if let Some(thread) = guard.as_ref() {
                    thread.set(handle, Some(wanted));
                }
            }
            None => {
                if let Some(thread) = guard.as_ref() {
                    thread.set(handle, None);
                }
            }
        }
        Ok(())
    }

    /// The foreground activation chain on the dock window itself, which keeps
    /// `WS_EX_NOACTIVATE` (`actions`, *Focusing a dock window*).
    fn focus_dock_window(&self, handle: NativeWindowHandle) -> PlatformResult<()> {
        actions::focus_dock_window(handle)
    }

    fn foreground_native_window(&self) -> Option<NativeWindowHandle> {
        dock_window::foreground_handle()
    }

    fn finish_popup(&self, dock: NativeWindowHandle, previous: Option<NativeWindowHandle>) {
        dock_window::finish_popup(dock, previous);
    }
}
