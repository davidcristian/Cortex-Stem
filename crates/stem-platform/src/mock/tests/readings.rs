use std::sync::Arc;

use parking_lot::Mutex;
use stem_core::{ClockFormat, PlatformEvent, PowerStatus};

use crate::Platform;
use crate::mock::MockPlatform;
use crate::mock::demo::demo_power;

#[test]
fn power_is_a_laptop_on_battery_in_the_demo_and_none_otherwise() {
    assert_eq!(
        MockPlatform::with_demo_scenario().power_status().unwrap(),
        PowerStatus {
            has_battery: true,
            percent: Some(76),
            charging: false,
            on_ac: false,
            saver: false,
        }
    );
    let empty = MockPlatform::new();
    assert_eq!(empty.power_status().unwrap(), PowerStatus::default());
    let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
    let sink_seen = Arc::clone(&seen);
    empty
        .start(Arc::new(move |ev| sink_seen.lock().push(ev)))
        .unwrap();
    // The power reading, then the regional format and the volume last:
    // no device on an empty platform.
    let seen = seen.lock();
    assert_eq!(
        seen[seen.len() - 3..],
        [
            PlatformEvent::PowerChanged(PowerStatus::default()),
            PlatformEvent::ClockFormatChanged(ClockFormat::default()),
            PlatformEvent::VolumeChanged(None),
        ]
    );
}

#[test]
fn set_power_reports_only_a_change_and_records_no_call() {
    let p = MockPlatform::with_demo_scenario();
    let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
    let sink_seen = Arc::clone(&seen);
    p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
        .unwrap();
    seen.lock().clear();

    // The same reading: nothing.
    assert!(!p.set_power(demo_power()));
    assert!(seen.lock().is_empty());

    // Plugged in and charging: one event, and the reading follows.
    let charging = PowerStatus {
        charging: true,
        on_ac: true,
        ..demo_power()
    };
    assert!(p.set_power(charging));
    assert_eq!(*seen.lock(), vec![PlatformEvent::PowerChanged(charging)]);
    assert_eq!(p.power_status().unwrap(), charging);
    assert!(!p.set_power(charging));
    assert_eq!(seen.lock().len(), 1);

    // A pushed reading is kept too.
    let saver = PowerStatus {
        saver: true,
        percent: Some(19),
        ..demo_power()
    };
    p.push(PlatformEvent::PowerChanged(saver));
    assert_eq!(p.power_status().unwrap(), saver);
    assert!(!p.set_power(saver));
    assert!(p.calls().is_empty());
}

#[test]
fn the_clock_format_is_unknown_unless_scripted_and_sent_after_the_snapshot() {
    let p = MockPlatform::with_demo_scenario();
    assert_eq!(p.clock_format().unwrap(), ClockFormat::default());
    let british = ClockFormat {
        locale: Some("en-GB".into()),
        hour12: Some(false),
    };
    // Scripted before the start: the start sequence ends with it, and
    // the volume after it.
    assert!(p.set_clock_format(british.clone()));
    let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
    let sink_seen = Arc::clone(&seen);
    p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
        .unwrap();
    let seen = seen.lock();
    assert_eq!(
        seen[seen.len() - 2],
        PlatformEvent::ClockFormatChanged(british.clone())
    );
    assert_eq!(p.clock_format().unwrap(), british);
}

#[test]
fn set_clock_format_reports_only_a_change_and_records_no_call() {
    let p = MockPlatform::with_demo_scenario();
    let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
    let sink_seen = Arc::clone(&seen);
    p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
        .unwrap();
    seen.lock().clear();

    // The same reading: nothing.
    assert!(!p.set_clock_format(ClockFormat::default()));
    assert!(seen.lock().is_empty());

    // A 12-hour region: one event, and the reading follows.
    let american = ClockFormat {
        locale: Some("en-US".into()),
        hour12: Some(true),
    };
    assert!(p.set_clock_format(american.clone()));
    assert_eq!(
        *seen.lock(),
        vec![PlatformEvent::ClockFormatChanged(american.clone())]
    );
    assert_eq!(p.clock_format().unwrap(), american);
    assert!(!p.set_clock_format(american.clone()));
    assert_eq!(seen.lock().len(), 1);

    // A custom 24-hour short time in the same region is a change too;
    // a pushed reading is kept as well.
    let custom = ClockFormat {
        hour12: Some(false),
        ..american
    };
    p.push(PlatformEvent::ClockFormatChanged(custom.clone()));
    assert_eq!(p.clock_format().unwrap(), custom);
    assert!(!p.set_clock_format(custom));
    assert!(p.calls().is_empty());
}
