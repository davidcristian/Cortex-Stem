use stem_core::settings::AutoHide;
use stem_platform::{AppBarGrant, AppBarRequest, NativeWindowHandle};

use super::bar::GrantChange;
use super::rules::grant_fits;
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

// -- Bar ---------------------------------------------------------------

const BOTTOM: AppBarRequest = AppBarRequest {
    monitor: MONITOR,
    edge: DockPosition::Bottom,
    thickness: 72,
};
/// The band above a 48 px taskbar.
const BAND: Rect = Rect::new(0, 960, 1920, 72);

fn grant(request: AppBarRequest, rect: Rect) -> AppBarGrant {
    AppBarGrant { request, rect }
}

/// [`MONITOR`] with its work area as the platform reports it.
fn monitor(work_area: Rect) -> MonitorInfo {
    MonitorInfo {
        id: stem_core::MonitorId("DISPLAY1".into()),
        name: "DISPLAY1".into(),
        bounds: MONITOR,
        work_area,
        scale_factor: 1.0,
        is_primary: true,
    }
}

/// A 48 px taskbar and the band above it reserved.
fn reserved() -> MonitorInfo {
    monitor(Rect::new(0, 0, 1920, 960))
}

/// A bar that asked for [`BOTTOM`] and holds [`BAND`].
fn holding() -> Bar {
    let mut bar = Bar::default();
    assert!(bar.want(Some(BOTTOM)));
    assert_eq!(
        bar.take_grant(Some(grant(BOTTOM, BAND))),
        GrantChange::Granted(grant(BOTTOM, BAND))
    );
    bar
}

#[test]
fn a_bar_sends_only_a_changed_request_and_keeps_a_band_that_still_fits() {
    let mut bar = Bar::default();
    assert!(!bar.want(None), "nothing to give back");
    assert!(bar.want(Some(BOTTOM)));
    assert!(!bar.want(Some(BOTTOM)), "the same request again");
    bar.take_grant(Some(grant(BOTTOM, BAND)));
    // Another thickness on the same edge: the band stays until the new
    // one arrives.
    let thicker = AppBarRequest {
        thickness: 96,
        ..BOTTOM
    };
    assert!(bar.want(Some(thicker)));
    assert_eq!(bar.grant, Some(grant(BOTTOM, BAND)));
    // Another edge: the band is dropped.
    let left = AppBarRequest {
        edge: DockPosition::Left,
        ..BOTTOM
    };
    assert!(bar.want(Some(left)));
    assert_eq!(bar.grant, None);
    assert_eq!(bar.released, None);
}

#[test]
fn a_bar_left_by_an_earlier_window_of_the_label_starts_over_for_the_new_one() {
    const OLD: NativeWindowHandle = NativeWindowHandle(0x10);
    const NEW: NativeWindowHandle = NativeWindowHandle(0x20);
    // The first sync of a window takes its handle; nothing to give back.
    let mut bar = Bar::default();
    assert_eq!(bar.rebind(Some(OLD)), None);
    assert_eq!(bar.handle, Some(OLD));
    assert!(bar.want(Some(BOTTOM)));
    bar.take_grant(Some(grant(BOTTOM, BAND)));
    // The same window again, or a handle that cannot be read: kept.
    assert_eq!(bar.rebind(Some(OLD)), None);
    assert_eq!(bar.rebind(None), None);
    assert_eq!(bar.handle, Some(OLD));
    assert_eq!(bar.sent, Some(BOTTOM));
    // A re-created window of the same label: the old band is given back
    // through the old handle and the bar asks again for the new one.
    assert_eq!(bar.rebind(Some(NEW)), Some(OLD));
    assert_eq!(bar.handle, Some(NEW));
    assert_eq!((bar.sent, bar.grant, bar.released), (None, None, None));
    assert!(bar.want(Some(BOTTOM)), "the request goes out again");
    // A bar that never asked for a band has nothing to give back.
    let mut idle = Bar {
        handle: Some(OLD),
        ..Bar::default()
    };
    assert_eq!(idle.rebind(Some(NEW)), None);
    assert_eq!(idle.handle, Some(NEW));
}

#[test]
fn a_release_keeps_the_band_as_the_one_given_back_until_the_next_request() {
    let mut bar = holding();
    assert!(bar.want(None));
    assert_eq!(bar.grant, None);
    assert_eq!(bar.released, Some(grant(BOTTOM, BAND)));
    assert!(bar.want(Some(BOTTOM)));
    assert_eq!(bar.released, None);
}

#[test]
fn only_a_band_for_the_request_in_effect_is_kept() {
    let mut bar = holding();
    assert_eq!(
        bar.take_grant(Some(grant(BOTTOM, BAND))),
        GrantChange::Same,
        "the same band reported again"
    );
    // A band reported for the old edge after the window asked for the
    // top edge counts as none.
    let top = AppBarRequest {
        edge: DockPosition::Top,
        ..BOTTOM
    };
    bar.want(Some(top));
    assert_eq!(bar.take_grant(Some(grant(BOTTOM, BAND))), GrantChange::Same);
    assert_eq!(bar.grant, None);
    let top_band = Rect::new(0, 0, 1920, 72);
    assert_eq!(
        bar.take_grant(Some(grant(top, top_band))),
        GrantChange::Granted(grant(top, top_band))
    );
    // The platform lost the band: gone, and the request stays sent (the
    // platform asks again).
    assert_eq!(bar.take_grant(None), GrantChange::Gone);
    assert_eq!(bar.sent, Some(top));
    assert_eq!(bar.take_grant(None), GrantChange::Same);
    // A band reported after a release is not kept.
    bar.want(None);
    assert_eq!(
        bar.take_grant(Some(grant(top, top_band))),
        GrantChange::Same
    );
    assert_eq!(bar.grant, None);
}

#[test]
fn the_window_goes_on_the_band_granted_for_its_place() {
    let mut bar = holding();
    assert_eq!(
        bar.band("dock", DockPosition::Bottom, &reserved()),
        Some(BAND)
    );
    // Not for another edge, nor another monitor.
    assert_eq!(bar.band("dock", DockPosition::Left, &reserved()), None);
    let other = MonitorInfo {
        bounds: Rect::new(1920, 0, 1920, 1080),
        ..reserved()
    };
    assert_eq!(bar.band("dock", DockPosition::Bottom, &other), None);
    // Nor once the band is gone.
    bar.take_grant(None);
    assert_eq!(bar.band("dock", DockPosition::Bottom, &reserved()), None);
}

#[test]
fn a_band_given_back_is_placed_on_until_the_work_area_grows_over_it() {
    let mut bar = holding();
    bar.want(None);
    // The work area as last reported still leaves the band out.
    assert_eq!(
        bar.band("dock", DockPosition::Bottom, &reserved()),
        Some(BAND)
    );
    // Another place forgets it.
    assert_eq!(bar.band("dock", DockPosition::Top, &reserved()), None);
    assert_eq!(bar.released, None);

    let mut bar = holding();
    bar.want(None);
    // The shell took it back: the work area reaches the taskbar again.
    let grown = monitor(Rect::new(0, 0, 1920, 1032));
    assert_eq!(bar.band("dock", DockPosition::Bottom, &grown), None);
    assert_eq!(bar.released, None);
    assert_eq!(bar.band("dock", DockPosition::Bottom, &reserved()), None);
}
