mod dock_windows;
mod identity;
mod readings;
mod shell;
mod volume;

use std::sync::Arc;

use parking_lot::Mutex;
use stem_core::{ClockFormat, PlatformEvent, WindowAction, WindowId};

use super::demo::{demo_monitor, demo_power, demo_volume, demo_windows};
use super::{MockCall, MockPlatform};
use crate::{Platform, PlatformError};

#[test]
fn start_emits_initial_snapshot_in_order() {
    let p = MockPlatform::with_demo_scenario();
    let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
    let sink_seen = Arc::clone(&seen);
    p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
        .unwrap();
    let seen = seen.lock();
    assert!(matches!(seen[0], PlatformEvent::MonitorsChanged(_)));
    assert!(matches!(seen[3], PlatformEvent::WindowsSnapshot(ref w) if w.len() == 6));
    assert_eq!(seen[4], PlatformEvent::ForegroundChanged(Some(WindowId(2))));
    assert_eq!(seen[5], PlatformEvent::PowerChanged(demo_power()));
    assert_eq!(
        seen[6],
        PlatformEvent::ClockFormatChanged(ClockFormat::default())
    );
    assert_eq!(seen[7], PlatformEvent::VolumeChanged(Some(demo_volume())));
    assert_eq!(seen.len(), 8);
    assert!(p.start(Arc::new(|_| {})).is_err());
}

#[test]
fn close_action_removes_window_and_records_call() {
    let p = MockPlatform::with_demo_scenario();
    p.window_action(WindowId(2), WindowAction::Close).unwrap();
    assert!(p.windows().unwrap().iter().all(|w| w.id != WindowId(2)));
    assert_eq!(p.foreground_window().unwrap(), None);
    assert_eq!(
        p.calls(),
        vec![MockCall::WindowAction(WindowId(2), WindowAction::Close)]
    );
    assert!(matches!(
        p.window_action(WindowId(2), WindowAction::Close),
        Err(PlatformError::WindowNotFound(_))
    ));
}

#[test]
fn demo_windows_keep_clear_of_the_bottom_left_and_right_panel_bands() {
    // Where a dock's panel rests on those edges, as in the TS mirror.
    const BAND: i32 = 250;
    let work = demo_monitor().work_area;
    for w in demo_windows() {
        let r = w.rect.unwrap();
        assert!(
            r.bottom() <= work.bottom() - BAND,
            "{}: bottom band",
            w.title
        );
        assert!(r.x >= work.x + BAND, "{}: left band", w.title);
        assert!(r.right() <= work.right() - BAND, "{}: right band", w.title);
    }
}
