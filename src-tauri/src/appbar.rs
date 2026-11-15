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
//!   window's edge side, in CSS px, taken when the page reports and scaled
//!   when the request is made, [`request_for`]); never the magnification
//!   headroom, which the window keeps: it reaches past the band into the
//!   work area, transparent and click-through there as before.
//! - The window goes with its edge side on the band the shell granted
//!   (`rules::appbar::window_rect`, in `windows::layout`) instead of on the
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
//! sink hops back to the main thread to place the window.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use stem_core::settings::{AutoHideMode, DockPosition};
use stem_core::{MonitorInfo, Rect, Settings};
use stem_platform::rules::appbar::work_area_excludes;
use stem_platform::{AppBarGrant, AppBarRequest, AppBarSink, NativeWindowHandle, PlatformError};
use tauri::{AppHandle, Manager, WebviewWindow};

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

/// One dock window's bar, as this module asked for it.
#[derive(Debug, Clone, Default)]
struct Bar {
    handle: Option<NativeWindowHandle>,
    /// The request last handed to the platform; `None`: no band.
    sent: Option<AppBarRequest>,
    /// The band the platform reported for `sent` (or for an earlier request
    /// on the same edge of the same monitor); `None` until it does.
    grant: Option<AppBarGrant>,
    /// The band given back last, placed on until the work area has grown
    /// over it ([`granted_band`]); dropped with the next request.
    released: Option<AppBarGrant>,
}

/// What decides a dock window's request ([`request_for`]); gathered by
/// `windows::app_bar_inputs` on the main thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarInputs {
    /// [`reservation_wanted`] as in effect.
    pub enabled: bool,
    /// Whether the window is shown (not hidden by the tray or the toggle).
    pub visible: bool,
    /// The bounds of the monitor the window was placed on, if known.
    pub monitor: Option<Rect>,
    /// The edge the window was placed on last.
    pub placed: Option<DockPosition>,
    /// The edge in effect (`settings.position`).
    pub edge: DockPosition,
    /// From the resting panel's far side to the window's edge side, CSS px,
    /// from the page's latest panel rect for the edge it is placed on.
    pub reserve_css: Option<i32>,
    /// The window's scale factor.
    pub scale: f64,
}

/// Whether the settings ask for bands: `system.reserveScreenSpace` on and
/// `autoHide.mode` off. Pure.
pub fn reservation_wanted(settings: &Settings) -> bool {
    settings.system.reserve_screen_space && settings.auto_hide.mode == AutoHideMode::Off
}

/// The band a dock window asks for, or `None`: nothing while bands are not
/// wanted, while the window is hidden, before it is placed on the edge in
/// effect or on a known monitor, or before its page reported a panel for
/// that edge. The thickness is `reserve_css` scaled and rounded up, so the
/// band never stops short of the panel. Pure.
pub fn request_for(inputs: &BarInputs) -> Option<AppBarRequest> {
    if !inputs.enabled || !inputs.visible || inputs.placed != Some(inputs.edge) {
        return None;
    }
    let monitor = inputs.monitor?;
    let css = inputs.reserve_css.filter(|&css| css > 0)?;
    let scale = if inputs.scale.is_finite() && inputs.scale > 0.0 {
        inputs.scale
    } else {
        1.0
    };
    let thickness = (f64::from(css) * scale)
        .ceil()
        .clamp(1.0, f64::from(i32::MAX)) as i32;
    Some(AppBarRequest {
        monitor,
        edge: inputs.edge,
        thickness,
    })
}

/// Whether `grant` is a band for the place `request` asks for: the same
/// edge of the same monitor (the thickness may still be on its way). Pure.
pub fn grant_fits(grant: &AppBarGrant, request: &AppBarRequest) -> bool {
    grant.request.edge == request.edge && grant.request.monitor == request.monitor
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
/// window placed (`windows::place`), shown or hidden, its page's panel
/// report, the setting. Cheap when nothing changed.
pub fn sync(app: &AppHandle, window: &WebviewWindow) {
    let Some(bars) = app.try_state::<AppBars>() else {
        return;
    };
    let enabled = bars.enabled.lock().unwrap_or(false);
    let inputs = crate::windows::app_bar_inputs(app, window, enabled);
    let wanted = request_for(&inputs);
    let label = window.label();
    let handle = {
        let mut map = bars.bars.lock();
        let bar = map.entry(label.to_owned()).or_default();
        if bar.handle.is_none() {
            bar.handle = crate::windows::native_handle(window);
        }
        if bar.sent == wanted {
            return;
        }
        bar.sent = wanted;
        match wanted {
            None => {
                if let Some(grant) = bar.grant.take() {
                    bar.released = Some(grant);
                }
            }
            Some(request) => {
                bar.released = None;
                if bar.grant.is_some_and(|grant| !grant_fits(&grant, &request)) {
                    bar.grant = None;
                }
            }
        }
        bar.handle
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
/// `windows::layout` places it on instead of the work area's edge: the band
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
    let bar = map.get_mut(label)?;
    let fits =
        |grant: &AppBarGrant| grant.request.edge == edge && grant.request.monitor == monitor.bounds;
    if let (Some(sent), Some(grant)) = (bar.sent, bar.grant) {
        return (fits(&grant) && grant_fits(&grant, &sent)).then_some(grant.rect);
    }
    let released = bar.released?;
    if !fits(&released) {
        bar.released = None;
        return None;
    }
    if work_area_excludes(monitor.work_area, released.rect, edge) {
        return Some(released.rect);
    }
    log::debug!(
        "dock window {label:?}: the band given back is free again; placing on the work area"
    );
    bar.released = None;
    None
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

/// Hand `wanted` for the window `label` to the platform.
fn request(
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
/// asked for that window; a new band lays the window out again, at the
/// next turn of the main thread's loop. A band that is gone moves nothing:
/// the window stays on the old band's edge side, the work area's edge once
/// the shell has given the band back, and the next monitor report lays it
/// out from the work area again.
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
        let grant = grant.filter(|grant| bar.sent.is_some_and(|sent| grant_fits(grant, &sent)));
        if bar.grant == grant {
            return;
        }
        bar.grant = grant;
        match grant {
            Some(grant) => {
                log::debug!(
                    "dock window {label:?}: the shell granted {:?} on the {:?} edge",
                    grant.rect,
                    grant.request.edge
                );
                Some(label.clone())
            }
            None => {
                log::debug!("dock window {label:?}: no band is reserved any more");
                None
            }
        }
    };
    if let Some(label) = relayout {
        crate::windows::relayout_later(app, &label);
    }
}

#[cfg(test)]
mod tests {
    use stem_core::settings::AutoHide;

    use super::*;

    const MONITOR: Rect = Rect::new(0, 0, 1920, 1080);

    fn inputs() -> BarInputs {
        BarInputs {
            enabled: true,
            visible: true,
            monitor: Some(MONITOR),
            placed: Some(DockPosition::Bottom),
            edge: DockPosition::Bottom,
            reserve_css: Some(72),
            scale: 1.0,
        }
    }

    #[test]
    fn bands_are_wanted_only_with_the_setting_on_and_auto_hide_off() {
        let mut settings = Settings::default();
        assert!(!reservation_wanted(&settings), "off by default");
        settings.system.reserve_screen_space = true;
        assert!(reservation_wanted(&settings));
        for mode in [AutoHideMode::Always, AutoHideMode::WhenOverlapped] {
            settings.auto_hide = AutoHide {
                mode,
                ..AutoHide::default()
            };
            assert!(!reservation_wanted(&settings), "{mode:?}");
        }
        settings.auto_hide.mode = AutoHideMode::Off;
        settings.system.reserve_screen_space = false;
        assert!(!reservation_wanted(&settings));
    }

    #[test]
    fn a_placed_visible_window_with_a_panel_asks_for_its_band() {
        assert_eq!(
            request_for(&inputs()),
            Some(AppBarRequest {
                monitor: MONITOR,
                edge: DockPosition::Bottom,
                thickness: 72,
            })
        );
        // At 150 %: 72 css px are 108 physical px; 75 are 112.5, rounded up.
        let scaled = BarInputs {
            scale: 1.5,
            ..inputs()
        };
        assert_eq!(request_for(&scaled).map(|r| r.thickness), Some(108));
        let odd = BarInputs {
            scale: 1.5,
            reserve_css: Some(75),
            ..inputs()
        };
        assert_eq!(request_for(&odd).map(|r| r.thickness), Some(113));
        // An unusable scale counts as 1.
        let broken = BarInputs {
            scale: f64::NAN,
            ..inputs()
        };
        assert_eq!(request_for(&broken).map(|r| r.thickness), Some(72));
    }

    #[test]
    fn no_band_while_disabled_hidden_unplaced_or_without_a_panel() {
        let cases = [
            BarInputs {
                enabled: false,
                ..inputs()
            },
            BarInputs {
                visible: false,
                ..inputs()
            },
            BarInputs {
                monitor: None,
                ..inputs()
            },
            BarInputs {
                placed: None,
                ..inputs()
            },
            // Moving to another edge: not there yet.
            BarInputs {
                placed: Some(DockPosition::Bottom),
                edge: DockPosition::Left,
                ..inputs()
            },
            BarInputs {
                reserve_css: None,
                ..inputs()
            },
            BarInputs {
                reserve_css: Some(0),
                ..inputs()
            },
        ];
        for case in cases {
            assert_eq!(request_for(&case), None, "{case:?}");
        }
    }

    #[test]
    fn a_band_fits_a_request_for_the_same_edge_of_the_same_monitor() {
        let request = AppBarRequest {
            monitor: MONITOR,
            edge: DockPosition::Bottom,
            thickness: 72,
        };
        let grant = AppBarGrant {
            request,
            rect: Rect::new(0, 960, 1920, 72),
        };
        assert!(grant_fits(&grant, &request));
        assert!(grant_fits(
            &grant,
            &AppBarRequest {
                thickness: 80,
                ..request
            }
        ));
        assert!(!grant_fits(
            &grant,
            &AppBarRequest {
                edge: DockPosition::Top,
                ..request
            }
        ));
        assert!(!grant_fits(
            &grant,
            &AppBarRequest {
                monitor: Rect::new(1920, 0, 1920, 1080),
                ..request
            }
        ));
    }
}
