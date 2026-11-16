use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use stem_core::Rect;

use crate::mock::demo::demo_monitor;
use crate::mock::{MockCall, MockPlatform};
use crate::{AppBarGrant, AppBarRequest, AppBarSink, DockBackdrop, NativeWindowHandle, Platform};

#[test]
fn app_bars_are_recorded_granted_on_the_work_area_edge_and_released() {
    use stem_core::settings::DockPosition;

    let p = MockPlatform::new();
    type Seen = Vec<(NativeWindowHandle, Option<AppBarGrant>)>;
    let grants: Arc<Mutex<Seen>> = Arc::default();
    let seen = Arc::clone(&grants);
    let sink: AppBarSink = Arc::new(move |handle, grant| seen.lock().push((handle, grant)));
    let monitor = demo_monitor();
    let bottom = AppBarRequest {
        monitor: monitor.bounds,
        edge: DockPosition::Bottom,
        thickness: 72,
    };
    let a = NativeWindowHandle(0xA);
    let b = NativeWindowHandle(0xB);

    p.set_dock_app_bar(a, Some(bottom), Arc::clone(&sink))
        .unwrap();
    // The demo monitor's work area stops 48 px short of its bottom.
    let band = Rect::new(0, 1392 - 72, 2560, 72);
    assert_eq!(
        grants.lock().clone(),
        vec![(
            a,
            Some(AppBarGrant {
                request: bottom,
                rect: band,
            })
        )]
    );
    // The same request again reports the same band again.
    p.set_dock_app_bar(a, Some(bottom), Arc::clone(&sink))
        .unwrap();
    assert_eq!(grants.lock().len(), 2);
    let seen = grants.lock().clone();
    assert_eq!(seen[1], seen[0]);

    // A monitor the script does not list: the band on its bounds.
    let elsewhere = AppBarRequest {
        monitor: Rect::new(-1920, 0, 1920, 1080),
        edge: DockPosition::Left,
        thickness: 80,
    };
    p.set_dock_app_bar(b, Some(elsewhere), Arc::clone(&sink))
        .unwrap();
    assert_eq!(
        grants.lock().last().copied(),
        Some((
            b,
            Some(AppBarGrant {
                request: elsewhere,
                rect: Rect::new(-1920, 0, 80, 1080),
            })
        ))
    );
    assert_eq!(p.app_bars(), vec![(a, bottom), (b, elsewhere)]);

    // A release reports nothing and drops the registration.
    p.set_dock_app_bar(a, None, Arc::clone(&sink)).unwrap();
    assert_eq!(grants.lock().len(), 3);
    assert_eq!(p.app_bars(), vec![(b, elsewhere)]);
    p.release_app_bars(Duration::from_millis(100)).unwrap();
    assert!(p.app_bars().is_empty());
    assert_eq!(
        p.calls(),
        vec![
            MockCall::SetDockAppBar(a, Some(bottom)),
            MockCall::SetDockAppBar(a, Some(bottom)),
            MockCall::SetDockAppBar(b, Some(elsewhere)),
            MockCall::SetDockAppBar(a, None),
            MockCall::ReleaseAppBars,
        ]
    );
}

#[test]
fn set_dock_backdrop_records_every_call_in_order() {
    let p = MockPlatform::new();
    let dock = NativeWindowHandle(0x1234);
    let acrylic = DockBackdrop {
        rect: Rect::new(800, 1300, 960, 96),
        corner_radius: 33,
        material: crate::BackdropMaterial::Acrylic,
        dark: true,
    };
    p.set_dock_backdrop(dock, Some(acrylic)).unwrap();
    p.set_dock_backdrop(dock, None).unwrap();
    assert_eq!(
        p.calls(),
        vec![
            MockCall::SetDockBackdrop(dock, Some(acrylic)),
            MockCall::SetDockBackdrop(dock, None),
        ]
    );
}

#[test]
fn focus_dock_window_is_recorded_and_leaves_the_foreground_alone() {
    let p = MockPlatform::with_demo_scenario();
    let before = p.foreground_window().unwrap();
    assert!(before.is_some());
    let dock = NativeWindowHandle(0x4321);
    p.focus_dock_window(dock).unwrap();
    assert_eq!(p.calls(), vec![MockCall::FocusDockWindow(dock)]);
    assert_eq!(p.foreground_window().unwrap(), before);
    // The mock has no native foreground window to report either, so the
    // app layer never takes a mock dock for the focused one.
    assert_eq!(p.foreground_native_window(), None);
}
