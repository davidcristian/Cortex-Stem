use std::sync::Arc;

use parking_lot::Mutex;
use stem_core::{PlatformEvent, ScrollDirection, VolumeStatus};

use crate::Platform;
use crate::mock::{MockCall, MockPlatform};

#[test]
fn the_volume_is_scripted_set_and_reported_only_when_it_changes() {
    let level = |percent, muted| Some(VolumeStatus { percent, muted });
    assert_eq!(MockPlatform::new().volume_status().unwrap(), None);
    let p = MockPlatform::with_demo_scenario();
    assert_eq!(p.volume_status().unwrap(), level(60, false));
    let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
    let sink_seen = Arc::clone(&seen);
    p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
        .unwrap();
    seen.lock().clear();

    // An outside change: one event when it differs, none otherwise,
    // and no call recorded.
    assert!(!p.change_volume(level(60, false)));
    assert!(p.change_volume(level(44, false)));
    assert_eq!(
        *seen.lock(),
        vec![PlatformEvent::VolumeChanged(level(44, false))]
    );
    assert!(p.calls().is_empty());
    seen.lock().clear();

    // Stem's own changes: recorded, applied, reported like any other.
    p.set_volume(46).unwrap();
    p.set_volume_muted(true).unwrap();
    assert_eq!(p.volume_status().unwrap(), level(46, true));
    // The same again changes nothing and reports nothing; a level past
    // 100 is 100.
    p.set_volume(46).unwrap();
    p.set_volume_muted(true).unwrap();
    p.set_volume(250).unwrap();
    assert_eq!(
        *seen.lock(),
        vec![
            PlatformEvent::VolumeChanged(level(46, false)),
            PlatformEvent::VolumeChanged(level(46, true)),
            PlatformEvent::VolumeChanged(level(100, true)),
        ]
    );
    assert_eq!(
        p.calls(),
        vec![
            MockCall::SetVolume(46),
            MockCall::SetVolumeMuted(true),
            MockCall::SetVolume(46),
            MockCall::SetVolumeMuted(true),
            MockCall::SetVolume(250),
        ]
    );

    // The device goes away: the actions fail (still recorded), the
    // reading is none.
    assert!(p.change_volume(None));
    assert_eq!(p.volume_status().unwrap(), None);
    assert!(p.set_volume(10).is_err());
    assert!(p.set_volume_muted(false).is_err());
    assert_eq!(p.calls().len(), 7);
    // A pushed reading is kept as well.
    p.push(PlatformEvent::VolumeChanged(level(5, false)));
    assert_eq!(p.volume_status().unwrap(), level(5, false));
}

#[test]
fn a_volume_step_applies_the_engines_rule_and_reports_only_a_change() {
    use ScrollDirection::{Next, Previous};
    let level = |percent, muted| Some(VolumeStatus { percent, muted });
    let p = MockPlatform::with_demo_scenario();
    let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
    let sink_seen = Arc::clone(&seen);
    p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
        .unwrap();
    seen.lock().clear();

    assert_eq!(p.step_volume(Previous).unwrap(), level(62, false));
    assert_eq!(p.step_volume(Next).unwrap(), level(60, false));
    // Muted at 99: up unmutes and stops at 100; then nothing.
    assert!(p.change_volume(level(99, true)));
    assert_eq!(p.step_volume(Previous).unwrap(), level(100, false));
    assert_eq!(p.step_volume(Previous).unwrap(), None);
    // Without a device: nothing, and no error.
    assert!(p.change_volume(None));
    assert_eq!(p.step_volume(Next).unwrap(), None);
    assert_eq!(
        *seen.lock(),
        vec![
            PlatformEvent::VolumeChanged(level(62, false)),
            PlatformEvent::VolumeChanged(level(60, false)),
            PlatformEvent::VolumeChanged(level(99, true)),
            PlatformEvent::VolumeChanged(level(100, false)),
            PlatformEvent::VolumeChanged(None),
        ]
    );
    assert_eq!(
        p.calls(),
        vec![
            MockCall::StepVolume(Previous),
            MockCall::StepVolume(Next),
            MockCall::StepVolume(Previous),
            MockCall::StepVolume(Previous),
            MockCall::StepVolume(Next),
        ]
    );
}
