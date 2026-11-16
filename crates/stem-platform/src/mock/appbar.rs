//! The mock's AppBars: one registration per window, granted at once.

use stem_core::{MonitorInfo, Rect};

use super::{MockCall, MockPlatform};
use crate::{AppBarGrant, AppBarRequest, AppBarSink, NativeWindowHandle, PlatformResult, rules};

impl MockPlatform {
    /// The AppBars registered now: each window with the request
    /// its band was granted for, in the order of their first registration.
    pub fn app_bars(&self) -> Vec<(NativeWindowHandle, AppBarRequest)> {
        self.state.lock().app_bars.clone()
    }

    /// [`Platform::set_dock_app_bar`](crate::Platform::set_dock_app_bar) on
    /// the scripted registrations.
    pub(super) fn apply_app_bar(
        &self,
        handle: NativeWindowHandle,
        request: Option<AppBarRequest>,
        sink: AppBarSink,
    ) -> PlatformResult<()> {
        let grant = {
            let mut s = self.state.lock();
            s.calls.push(MockCall::SetDockAppBar(handle, request));
            let held = s.app_bars.iter().position(|(h, _)| *h == handle);
            let granted = match (request, held) {
                (None, Some(i)) => {
                    s.app_bars.remove(i);
                    None
                }
                (None, None) => None,
                (Some(request), Some(i)) => {
                    s.app_bars[i].1 = request;
                    Some(request)
                }
                (Some(request), None) => {
                    s.app_bars.push((handle, request));
                    Some(request)
                }
            };
            granted.map(|request| AppBarGrant {
                request,
                rect: mock_band(&s.monitors, &request),
            })
        };
        if let Some(grant) = grant {
            sink(handle, Some(grant));
        }
        Ok(())
    }
}

/// The band the mock's shell grants for `request`: on the request's edge of
/// the work area of the scripted monitor whose bounds it names (as if the
/// stock taskbar, or another bar, held the rest), or of the bounds
/// themselves for a monitor the script does not list; cut to the request's
/// thickness by the same rule as the real platform (`rules::appbar`).
pub(super) fn mock_band(monitors: &[MonitorInfo], request: &AppBarRequest) -> Rect {
    let area = monitors
        .iter()
        .find(|m| m.bounds == request.monitor)
        .map_or(request.monitor, |m| m.work_area);
    rules::appbar::set_rect(area, request)
}
