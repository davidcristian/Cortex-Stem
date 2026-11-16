//! The requests to `Platform::set_dock_app_bar` and the bands the
//! platform reports back.

use std::sync::Arc;

use stem_platform::{AppBarGrant, AppBarRequest, AppBarSink, NativeWindowHandle, PlatformError};
use tauri::{AppHandle, Manager};

use super::AppBars;
use super::bar::GrantChange;
use crate::AppService;

/// Hand `wanted` for the window `label` to the platform.
pub(super) fn request(
    app: &AppHandle,
    label: &str,
    handle: Option<NativeWindowHandle>,
    wanted: Option<AppBarRequest>,
) {
    let Some(handle) = handle else {
        log::debug!("dock window {label:?}: no native handle; no screen space reserved");
        return;
    };
    let Some(service) = app.try_state::<AppService>() else {
        return;
    };
    match service
        .0
        .platform()
        .set_dock_app_bar(handle, wanted, sink(app))
    {
        Ok(()) => {}
        Err(PlatformError::Unsupported(task)) => {
            static LOGGED: std::sync::Once = std::sync::Once::new();
            LOGGED.call_once(|| {
                log::info!("reserve screen space is not supported by this platform ({task})");
            });
        }
        Err(e) => log::warn!("dock window {label:?}: the screen space request failed: {e}"),
    }
}

/// Where the platform reports the bands.
fn sink(app: &AppHandle) -> AppBarSink {
    let app = app.clone();
    Arc::new(move |handle, grant| on_grant(&app, handle, grant))
}

/// The platform reported the band of the window `handle` (its own thread,
/// or the caller's for the mock). Remembered when it answers what was last
/// asked for that window ([`Bar::take_grant`]); a new band lays the window
/// out again, at the next turn of the main thread's loop. A band that is
/// gone moves nothing: the window stays on the old band's edge side, the
/// work area's edge once the shell has given the band back, and the next
/// monitor report lays it out from the work area again; the platform keeps
/// asking for the band and reports it once it holds it again.
///
/// [`Bar::take_grant`]: super::bar::Bar::take_grant
fn on_grant(app: &AppHandle, handle: NativeWindowHandle, grant: Option<AppBarGrant>) {
    let Some(bars) = app.try_state::<AppBars>() else {
        return;
    };
    let relayout = {
        let mut map = bars.bars.lock();
        let Some((label, bar)) = map.iter_mut().find(|(_, bar)| bar.handle == Some(handle)) else {
            log::debug!(
                "a band for a window that is not a dock window ({:#x})",
                handle.0
            );
            return;
        };
        match bar.take_grant(grant) {
            GrantChange::Same => None,
            GrantChange::Granted(grant) => {
                log::debug!(
                    "dock window {label:?}: the shell granted {:?} on the {:?} edge",
                    grant.rect,
                    grant.request.edge
                );
                Some(label.clone())
            }
            GrantChange::Gone => {
                log::debug!(
                    "dock window {label:?}: no band is reserved any more (the platform asks \
                     for it again)"
                );
                None
            }
        }
    };
    if let Some(label) = relayout {
        crate::windows::relayout_later(app, &label);
    }
}
