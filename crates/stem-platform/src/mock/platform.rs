//! [`Platform`] for [`MockPlatform`]: every method on the scripted state.

use std::sync::Arc;
use std::time::Duration;

use stem_core::{
    AppId, AppIdentity, ClockFormat, IconKey, LaunchTarget, MonitorInfo, PlatformEvent,
    PowerStatus, RawWindow, RecycleBinState, ScrollDirection, ShellAction, ThemeInfo, VolumeStatus,
    WindowAction, WindowId,
};

use super::icon::{color_for, solid_png};
use super::identity::{file_stem, identity_for, identity_for_aumid};
use super::{MockCall, MockPlatform};
use crate::{
    AppBarRequest, AppBarSink, DockBackdrop, DockWindowOptions, EventSink, HitRegion,
    NativeWindowHandle, Platform, PlatformError, PlatformResult, rules,
};

impl Platform for MockPlatform {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn start(&self, sink: EventSink) -> PlatformResult<()> {
        let initial = {
            let mut s = self.state.lock();
            if s.sink.is_some() {
                return Err(PlatformError::Other("mock platform already started".into()));
            }
            s.sink = Some(Arc::clone(&sink));
            [
                PlatformEvent::MonitorsChanged(s.monitors.clone()),
                PlatformEvent::ThemeChanged(s.theme.clone()),
                PlatformEvent::RecycleBinChanged(s.recycle_bin),
                PlatformEvent::WindowsSnapshot(s.windows.clone()),
                PlatformEvent::ForegroundChanged(s.foreground),
                // Once after the snapshot, as the real events thread does.
                PlatformEvent::PowerChanged(s.power),
                PlatformEvent::ClockFormatChanged(s.clock_format.clone()),
                // The volume thread's first reading, last.
                PlatformEvent::VolumeChanged(s.volume),
            ]
        };
        for ev in initial {
            sink(ev);
        }
        Ok(())
    }

    fn stop(&self) {
        self.state.lock().sink = None;
    }

    fn windows(&self) -> PlatformResult<Vec<RawWindow>> {
        Ok(self.state.lock().windows.clone())
    }

    fn foreground_window(&self) -> PlatformResult<Option<WindowId>> {
        Ok(self.state.lock().foreground)
    }

    fn window_action(&self, id: WindowId, action: WindowAction) -> PlatformResult<()> {
        self.act_on_window(id, action)
    }

    fn identify_window(&self, id: WindowId) -> PlatformResult<AppIdentity> {
        let raw = self
            .state
            .lock()
            .windows
            .iter()
            .find(|w| w.id == id)
            .cloned();
        let raw = raw.ok_or(PlatformError::WindowNotFound(id))?;
        Ok(identity_for(&raw))
    }

    /// A `StoreApp` target is the same identity as the app's windows
    /// ([`identity_for_aumid`]), as on Windows, where both come from the
    /// AppsFolder; the rest are named after the file or the URI.
    fn identify_target(&self, target: &LaunchTarget) -> PlatformResult<AppIdentity> {
        let (id, name) = match target {
            LaunchTarget::Exe { path, .. } | LaunchTarget::Shortcut { path } => {
                (AppId::for_exe(path), file_stem(path))
            }
            LaunchTarget::StoreApp { app_user_model_id } => {
                return Ok(identity_for_aumid(app_user_model_id));
            }
            LaunchTarget::Uri { uri } => (AppId(format!("uri:{uri}")), uri.clone()),
        };
        Ok(AppIdentity {
            icon_key: Some(IconKey(format!("mock:{}", id.as_str()))),
            id,
            display_name: name,
            launch: Some(target.clone()),
        })
    }

    fn launch(&self, target: &LaunchTarget) -> PlatformResult<()> {
        self.record(MockCall::Launch(target.clone()));
        Ok(())
    }

    fn icon_png(&self, key: &IconKey, size: u32) -> PlatformResult<Option<Vec<u8>>> {
        if key.is_shell() {
            return Ok(None);
        }
        Ok(Some(solid_png(
            size.clamp(8, 256),
            color_for(key.0.as_str()),
        )))
    }

    fn monitors(&self) -> PlatformResult<Vec<MonitorInfo>> {
        Ok(self.state.lock().monitors.clone())
    }

    fn theme(&self) -> PlatformResult<ThemeInfo> {
        Ok(self.state.lock().theme.clone())
    }

    fn recycle_bin(&self) -> PlatformResult<RecycleBinState> {
        Ok(self.state.lock().recycle_bin)
    }

    /// Recorded; then every plain absolute path counts as existing
    /// (`rules::recycle` with nothing missing) and is added to the bin's
    /// item count. Silently, like the real bin: no `RecycleBinChanged` is
    /// pushed, `recycle_bin` reports it (the service asks right after).
    fn delete_to_recycle_bin(&self, paths: &[String]) -> PlatformResult<()> {
        self.record(MockCall::DeleteToRecycleBin(paths.to_vec()));
        let mut s = self.state.lock();
        if let Some(message) = &s.recycle_failure {
            return Err(PlatformError::Other(message.clone()));
        }
        let plan = rules::recycle::plan_recycle(paths, |_| true);
        if plan.recycle.is_empty() {
            return Err(PlatformError::Other(rules::recycle::nothing_to_recycle(
                &plan,
            )));
        }
        s.recycle_bin.item_count += plan.recycle.len() as u64;
        Ok(())
    }

    /// The scripted reading (no battery unless the demo or
    /// [`MockPlatform::set_power`] gave one); nothing is recorded.
    fn power_status(&self) -> PlatformResult<PowerStatus> {
        Ok(self.state.lock().power)
    }

    /// The scripted regional format (nothing known unless
    /// [`MockPlatform::set_clock_format`] or a pushed event gave one);
    /// nothing is recorded.
    fn clock_format(&self) -> PlatformResult<ClockFormat> {
        Ok(self.state.lock().clock_format.clone())
    }

    /// The scripted device's volume (none unless the demo,
    /// [`MockPlatform::change_volume`] or a pushed event gave one); nothing
    /// is recorded. Answers before `start` too.
    fn volume_status(&self) -> PlatformResult<Option<VolumeStatus>> {
        Ok(self.state.lock().volume)
    }

    /// Recorded; then the scripted level becomes `percent` (at most 100),
    /// reported like an outside change. Fails without a scripted device.
    fn set_volume(&self, percent: u8) -> PlatformResult<()> {
        self.record(MockCall::SetVolume(percent));
        self.change_output(|v| v.percent = percent.min(100))
    }

    /// Recorded; then the scripted mute becomes `muted`, reported like an
    /// outside change. Fails without a scripted device.
    fn set_volume_muted(&self, muted: bool) -> PlatformResult<()> {
        self.record(MockCall::SetVolumeMuted(muted));
        self.change_output(|v| v.muted = muted)
    }

    /// Recorded; then the scripted device's volume becomes what
    /// `stem_core::engine::volume_step` makes of it, under the state's lock
    /// as the real thread does it in one message, reported like an outside
    /// change. `None` without a scripted device or when nothing changes.
    fn step_volume(&self, direction: ScrollDirection) -> PlatformResult<Option<VolumeStatus>> {
        self.step_output(direction)
    }

    fn shell_action(&self, action: ShellAction) -> PlatformResult<()> {
        self.record(MockCall::Shell(action));
        if action == ShellAction::EmptyRecycleBin {
            self.push(PlatformEvent::RecycleBinChanged(RecycleBinState::default()));
        }
        Ok(())
    }

    /// The scripted state (`false` unless [`MockPlatform::script_taskbar_auto_hide`]
    /// or a set changed it); the read is recorded.
    fn taskbar_auto_hide(&self) -> PlatformResult<bool> {
        self.record(MockCall::TaskbarAutoHide);
        Ok(self.state.lock().taskbar_auto_hide)
    }

    /// Recorded and applied to the scripted state; nothing real changes.
    fn set_taskbar_auto_hide(&self, enabled: bool) -> PlatformResult<()> {
        self.record(MockCall::SetTaskbarAutoHide(enabled));
        self.state.lock().taskbar_auto_hide = enabled;
        Ok(())
    }

    /// Recorded; answers the scripted value at once (no box, no wait).
    fn ask_yes_no(&self, title: &str, text: &str) -> PlatformResult<bool> {
        self.record(MockCall::AskYesNo {
            title: title.to_owned(),
            text: text.to_owned(),
        });
        Ok(self.state.lock().yes_no_answer)
    }

    fn prepare_dock_window(
        &self,
        handle: NativeWindowHandle,
        options: DockWindowOptions,
    ) -> PlatformResult<()> {
        self.record(MockCall::PrepareDockWindow(handle, options));
        Ok(())
    }

    /// Recorded only: a browser or a mock run has no OS hit-testing.
    fn set_dock_hit_region(
        &self,
        handle: NativeWindowHandle,
        region: Option<HitRegion>,
    ) -> PlatformResult<()> {
        self.record(MockCall::SetDockHitRegion(handle, region));
        Ok(())
    }

    /// Recorded, and kept in [`MockPlatform::app_bars`]; nothing real is
    /// reserved and the scripted work areas do not change. A request is
    /// granted at once, without the real platform's wait, and the grant
    /// ([`mock_band`](super::appbar::mock_band)) goes to `sink` synchronously,
    /// on the calling thread, after every request, the same one again included, as the real
    /// platform reports after every request it applied. A release reports
    /// nothing, like the real platform.
    fn set_dock_app_bar(
        &self,
        handle: NativeWindowHandle,
        request: Option<AppBarRequest>,
        sink: AppBarSink,
    ) -> PlatformResult<()> {
        self.apply_app_bar(handle, request, sink)
    }

    /// Recorded; every registration is dropped at once.
    fn release_app_bars(&self, _timeout: Duration) -> PlatformResult<()> {
        let mut s = self.state.lock();
        s.calls.push(MockCall::ReleaseAppBars);
        s.app_bars.clear();
        Ok(())
    }

    /// Recorded only: a browser or a mock run has no compositor material.
    fn set_dock_backdrop(
        &self,
        handle: NativeWindowHandle,
        backdrop: Option<DockBackdrop>,
    ) -> PlatformResult<()> {
        self.record(MockCall::SetDockBackdrop(handle, backdrop));
        Ok(())
    }

    /// Recorded only: a mock run has no foreground to give, so the call
    /// succeeds and nothing else changes (the scripted foreground window
    /// stays what it was, like the real events thread, which ignores
    /// foreground reports for our own windows).
    fn focus_dock_window(&self, handle: NativeWindowHandle) -> PlatformResult<()> {
        self.record(MockCall::FocusDockWindow(handle));
        Ok(())
    }
}
