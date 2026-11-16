//! The record of the mock's side-effecting calls, for test assertions.

use stem_core::{LaunchTarget, ScrollDirection, ShellAction, WindowAction, WindowId};

use super::MockPlatform;
use crate::{AppBarRequest, DockBackdrop, DockWindowOptions, HitRegion, NativeWindowHandle};

/// A record of a side-effecting call, for test assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockCall {
    WindowAction(WindowId, WindowAction),
    Launch(LaunchTarget),
    /// `delete_to_recycle_bin` with these paths, as given.
    DeleteToRecycleBin(Vec<String>),
    Shell(ShellAction),
    PrepareDockWindow(NativeWindowHandle, DockWindowOptions),
    SetDockHitRegion(NativeWindowHandle, Option<HitRegion>),
    /// `set_dock_backdrop` with these arguments.
    SetDockBackdrop(NativeWindowHandle, Option<DockBackdrop>),
    /// `taskbar_auto_hide` was read; recorded so tests can check
    /// that the live state is read before it is set.
    TaskbarAutoHide,
    /// `set_taskbar_auto_hide` with this value.
    SetTaskbarAutoHide(bool),
    /// `ask_yes_no` with this title and text; the answer is the
    /// scripted one.
    AskYesNo {
        title: String,
        text: String,
    },
    /// `set_dock_app_bar` with this window and request.
    SetDockAppBar(NativeWindowHandle, Option<AppBarRequest>),
    /// `release_app_bars`.
    ReleaseAppBars,
    /// `focus_dock_window` with this window.
    FocusDockWindow(NativeWindowHandle),
    /// `set_volume` with this percent, as given.
    SetVolume(u8),
    /// `set_volume_muted` with this value.
    SetVolumeMuted(bool),
    /// `step_volume` in this direction, whatever it did.
    StepVolume(ScrollDirection),
}

impl MockPlatform {
    /// Calls recorded so far (oldest first).
    pub fn calls(&self) -> Vec<MockCall> {
        self.state.lock().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.state.lock().calls.clear();
    }

    pub(super) fn record(&self, call: MockCall) {
        self.state.lock().calls.push(call);
    }
}
