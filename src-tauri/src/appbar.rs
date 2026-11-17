//! Reserve screen space: `system.reserveScreenSpace`
//! registers every visible dock window as an AppBar, so the shell takes a
//! band at the dock's edge out of its monitor's work area and maximised
//! windows stop short of the dock (ADR-0010).
//!
//! The rules, all decided here and in `stem_platform::rules::appbar`:
//! - A band is reserved only while the setting is on **and**
//!   `autoHide.mode` is `off` ([`reservation_wanted`]): a dock that hides
//!   itself reserves nothing, so a hidden dock never leaves an empty band.
//!   A window hidden by the tray or the toggle shortcut gives its band back
//!   too, and takes it again when it is shown. Either setting changing
//!   registers or removes the bands without a restart.
//! - One bar per dock window, on its edge (`settings.position`) of its
//!   monitor's **bounds**; the shell moves it past the stock taskbar and
//!   other bars on that edge. Its thickness is the panel's resting
//!   thickness plus its margin to the edge, from the window's latest
//!   `set_dock_panel_rect` (the distance from the panel's far side to the
//!   viewport's edge side, in CSS px, taken when the page reports and
//!   scaled when the request is made, [`request_for`]). It is read against
//!   the viewport the report carries (`windows::PanelReport`): that
//!   distance is what the re-anchored rect keeps in the window's viewport
//!   of any moment, so a report handled after a resize the page had not
//!   seen yet (a drop, a smaller icon size) gives the right thickness at
//!   once. (Earlier it was read against the window's viewport when the
//!   report was handled, and such a report had to leave the thickness as it
//!   was until the page's next one; a report measured for another edge is
//!   now dropped before it gets here.) Never the magnification headroom,
//!   which the window keeps: it reaches past the band into the work area,
//!   transparent and click-through there as before.
//! - The window goes with its edge side on the band the shell granted
//!   (`rules::appbar::window_rect`, in `windows::layout::layout`) instead of on the
//!   work area's edge, which is the band's far side once the band is
//!   reserved. A band is used only while it answers the request in effect
//!   (same edge, same monitor bounds): a grant for an old edge or monitor
//!   is never placed on.
//! - A release (setting off, auto-hide on, the window hidden or closed, the
//!   page's panel rect withdrawn on a position change until the page
//!   reports for the new edge) gives the band back at once. The window
//!   stays on the band's edge side, which is where the work area's edge is
//!   once the band is gone: until the work area the platform reports has
//!   grown over the band (`rules::appbar::work_area_excludes`), a layout
//!   still places it there, so it never moves inwards by the band's
//!   thickness and back.
//! - The quit gives every band back ([`on_exit`], bounded).
//!
//! Requests go to `Platform::set_dock_app_bar`, which only queues them: the
//! Windows platform sends every `SHAppBarMessage` on a thread of its own and
//! reports the band through the sink, from that thread. So this module
//! runs its requests on the main thread, where the window getters are
//! direct calls, without ever sending a message to the shell there; the
//! sink hops back to the main thread to place the window. The platform
//! reports the band after every request it applied and keeps asking for a
//! band it could not take, so a request is sent once, when it changes.

mod bar;
mod platform;
mod rules;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::time::Duration;

use parking_lot::Mutex;
use stem_core::settings::{AutoHideMode, DockPosition};
use stem_core::{MonitorInfo, Rect, Settings};
use stem_platform::PlatformError;
use tauri::{AppHandle, Manager, WebviewWindow};

use bar::Bar;
use platform::request;
pub use rules::BarInputs;
use rules::{request_for, reservation_wanted};

use crate::AppService;

/// How long the quit waits for the shell to take the bands back.
const RELEASE_TIMEOUT: Duration = Duration::from_millis(1000);

/// Managed state: whether bands are wanted, and each dock window's bar.
#[derive(Debug, Default)]
pub struct AppBars {
    /// [`reservation_wanted`] as currently in effect.
    enabled: Mutex<Option<bool>>,
    /// By dock window label.
    bars: Mutex<HashMap<String, Bar>>,
}

fn describe(settings: &Settings) -> &'static str {
    match (
        settings.system.reserve_screen_space,
        settings.auto_hide.mode == AutoHideMode::Off,
    ) {
        (false, _) => "off",
        (true, true) => "on",
        (true, false) => "on, but the dock auto-hides: nothing is reserved",
    }
}

/// Take the settings in effect at start, before the dock windows exist;
/// their first panel reports then make the requests. `setup`, after the
/// state is managed.
pub fn init(app: &AppHandle, settings: &Settings) {
    if let Some(bars) = app.try_state::<AppBars>() {
        *bars.enabled.lock() = Some(reservation_wanted(settings));
    }
    log::info!("reserve screen space: {}", describe(settings));
}

/// The settings changed: when [`reservation_wanted`] flips, ask for or give
/// back every dock window's band. Any thread; the work hops to the main
/// thread (after the window work `windows` scheduled for the same change).
pub fn on_settings_changed(app: &AppHandle, settings: &Settings) {
    let Some(bars) = app.try_state::<AppBars>() else {
        return;
    };
    let enabled = reservation_wanted(settings);
    {
        let mut current = bars.enabled.lock();
        if *current == Some(enabled) {
            return;
        }
        *current = Some(enabled);
    }
    log::info!("reserve screen space: {}", describe(settings));
    let handle = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        for window in crate::windows::dock_windows(&handle) {
            sync(&handle, &window);
        }
    });
    if let Err(e) = scheduled {
        log::warn!("could not schedule the screen space change: {e}");
    }
}

/// Bring the band of `window` in line with what it should reserve now
/// ([`request_for`]): hand a changed request to the platform, which queues
/// it. Called on the main thread whenever an input may have changed: the
/// window placed (`windows::layout::place`), shown or hidden, its page's panel
/// report, the setting. Cheap when nothing changed. The bar always speaks
/// for the window's handle of the moment ([`Bar::rebind`]): an entry a
/// destroyed window of the same label left behind gives its band back and
/// starts over, so a re-created dock window never inherits a dead handle.
pub fn sync(app: &AppHandle, window: &WebviewWindow) {
    let Some(bars) = app.try_state::<AppBars>() else {
        return;
    };
    let enabled = bars.enabled.lock().unwrap_or(false);
    let inputs = crate::windows::app_bar_inputs(app, window, enabled);
    let wanted = request_for(&inputs);
    let label = window.label();
    let current = crate::windows::native_handle(window);
    let (stale, send) = {
        let mut map = bars.bars.lock();
        let bar = map.entry(label.to_owned()).or_default();
        let stale = bar.rebind(current);
        (stale, bar.want(wanted).then_some(bar.handle))
    };
    if let Some(old) = stale {
        log::debug!(
            "dock window {label:?}: its bar belonged to an earlier window ({:#x}); that band is given back",
            old.0
        );
        request(app, label, Some(old), None);
    }
    let Some(handle) = send else {
        return;
    };
    match wanted {
        Some(request) => log::debug!(
            "dock window {label:?}: asking for {} px on the {:?} edge of {:?}",
            request.thickness,
            request.edge,
            request.monitor
        ),
        None => log::debug!("dock window {label:?}: no screen space reserved"),
    }
    request(app, label, handle, wanted);
}

/// The dock window `label` is about to be closed (its monitor went away, or
/// the mode went back to the primary only): give its band back and forget
/// it. Main thread.
pub fn forget(app: &AppHandle, label: &str) {
    let Some(bars) = app.try_state::<AppBars>() else {
        return;
    };
    let Some(bar) = bars.bars.lock().remove(label) else {
        return;
    };
    if bar.sent.is_some() {
        log::debug!("dock window {label:?}: closing; its band is given back");
        request(app, label, bar.handle, None);
    }
}

/// The band the dock window `label` goes on for `edge` of `monitor`, which
/// `windows::layout::layout` places it on instead of the work area's edge: the band
/// the shell granted for exactly that place while it is asked for, or the
/// band given back last for that place while `monitor`'s work area, as the
/// platform last reported it, still leaves it out (the shell has not taken
/// it back yet, or the report has not arrived). Main thread.
pub fn granted_band(
    app: &AppHandle,
    label: &str,
    edge: DockPosition,
    monitor: &MonitorInfo,
) -> Option<Rect> {
    let bars = app.try_state::<AppBars>()?;
    let mut map = bars.bars.lock();
    map.get_mut(label)?.band(label, edge, monitor)
}

/// Give every band back before the app ends (`RunEvent::Exit`), on the
/// calling thread (the main thread), bounded by [`RELEASE_TIMEOUT`]: the
/// platform keeps handling the messages the shell sends to the dock
/// windows while it waits. Before the taskbar restore (`taskbar::on_exit`),
/// so the shell has no bar of ours left to notify when the taskbar's
/// auto-hide changes.
pub fn on_exit(app: &AppHandle) {
    let Some(service) = app.try_state::<AppService>() else {
        return;
    };
    let held = app
        .try_state::<AppBars>()
        .is_some_and(|bars| bars.bars.lock().values().any(|bar| bar.sent.is_some()));
    match service.0.platform().release_app_bars(RELEASE_TIMEOUT) {
        Ok(()) if held => log::info!("reserve screen space: every band given back"),
        Ok(()) | Err(PlatformError::Unsupported(_)) => {}
        Err(e) => log::warn!("reserve screen space: the bands could not be given back: {e}"),
    }
}
