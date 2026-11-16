use pretty_assertions::assert_eq;

use super::*;

#[test]
fn the_sdk_values_are_the_documented_ones() {
    // mmdeviceapi.h: eRender = 0, eConsole = 0; winerror.h: ERROR_NOT_FOUND
    // is 1168 (0x490) and HRESULT_FROM_WIN32 puts it in FACILITY_WIN32.
    assert_eq!(E_RENDER, 0);
    assert_eq!(E_CONSOLE, 0);
    assert_eq!(E_NOTFOUND, 0x8007_0000 | 1168);
}

#[test]
fn the_percent_is_the_scalar_times_100_rounded_and_clamped() {
    for (scalar, percent) in [
        (0.0, 0),
        (1.0, 100),
        (0.45, 45),
        // What a level set by steps of 0.02 reads back as.
        (0.439_999_98, 44),
        (0.460_000_04, 46),
        (0.004, 0),
        (0.006, 1),
        (0.994, 99),
        (0.996, 100),
        // Exactly half way (0.125 is exact in binary): away from zero.
        (0.125, 13),
        // Out of range, or not a number: clamped, or nothing.
        (-0.2, 0),
        (1.7, 100),
        (f32::INFINITY, 100),
        (f32::NEG_INFINITY, 0),
        (f32::NAN, 0),
    ] {
        assert_eq!(percent_of(scalar), percent, "{scalar}");
    }
    assert_eq!(
        status_of(0.6, true),
        VolumeStatus {
            percent: 60,
            muted: true
        }
    );
}

#[test]
fn every_percent_survives_the_round_trip_through_the_scalar() {
    for percent in 0..=100u8 {
        assert_eq!(percent_of(scalar_of(percent)), percent);
    }
    assert_eq!(scalar_of(0), 0.0);
    assert_eq!(scalar_of(100), 1.0);
    assert_eq!(scalar_of(250), 1.0);
}

#[test]
fn only_the_console_render_role_is_followed() {
    // eRender/eCapture = 0/1; eConsole/eMultimedia/eCommunications = 0/1/2.
    assert!(is_console_render(0, 0));
    for (flow, role) in [(0, 1), (0, 2), (1, 0), (1, 1), (2, 0)] {
        assert!(!is_console_render(flow, role), "{flow} {role}");
    }
    assert!(is_no_device(0x8007_0490));
    // AUDCLNT_E_SERVICE_NOT_RUNNING, E_ACCESSDENIED: worth a retry.
    assert!(!is_no_device(0x8889_0010));
    assert!(!is_no_device(0x8007_0005));
}

#[test]
fn a_change_after_a_quiet_interval_is_reported_at_once() {
    let mut t = Throttle::new();
    assert!(t.notify(1_000));
    t.reported(1_000);
    assert_eq!(t.deadline(), None);
    // 50 ms later or more: at once again.
    assert!(t.notify(1_050));
    t.reported(1_050);
    assert!(t.notify(5_000));
}

#[test]
fn a_burst_is_reported_once_at_the_end_of_the_interval_with_the_last_value() {
    let mut t = Throttle::new();
    assert!(t.notify(0));
    t.reported(0);
    // A slider drag: a notification every 5 ms. Only the first after
    // the report arms the deadline; the rest wait for it.
    assert!(!t.notify(5));
    assert_eq!(t.deadline(), Some(50));
    for ms in (10..50).step_by(5) {
        assert!(!t.notify(ms));
        assert_eq!(t.deadline(), Some(50));
    }
    assert!(!t.take_due(49));
    assert!(t.take_due(50));
    assert_eq!(t.deadline(), None);
    t.reported(50);
    // The drag goes on: the next report is due 50 ms after that one.
    assert!(!t.notify(52));
    assert_eq!(t.deadline(), Some(100));
    // A late wake-up still reports, once.
    assert!(t.take_due(130));
    assert!(!t.take_due(131));
    t.reported(130);
    assert!(!t.notify(140));
    assert_eq!(t.deadline(), Some(180));
}

#[test]
fn reports_are_at_least_an_interval_apart_and_the_last_change_is_never_lost() {
    // A notification every 7 ms for 300 ms, a report whenever allowed:
    // every report at least 50 ms after the one before, and one at or
    // after the last notification.
    let mut t = Throttle::new();
    let mut reports = Vec::new();
    for now in 0..400u64 {
        if now <= 300 && now % 7 == 0 && t.notify(now) {
            t.reported(now);
            reports.push(now);
        }
        if t.take_due(now) {
            t.reported(now);
            reports.push(now);
        }
    }
    assert!(
        reports
            .windows(2)
            .all(|w| w[1] - w[0] >= Throttle::INTERVAL_MS)
    );
    assert!(reports.last().is_some_and(|&last| last >= 294));
    assert_eq!(t.deadline(), None);
}

#[test]
fn a_reading_that_changed_nothing_does_not_start_an_interval() {
    let mut t = Throttle::new();
    assert!(t.notify(0));
    // Read, but the filter dropped it: no `reported`.
    assert!(t.notify(10));
    t.reported(10);
    assert!(!t.notify(20));
}

#[test]
fn the_filter_reports_the_first_reading_and_then_only_changes() {
    let level = |percent, muted| Some(VolumeStatus { percent, muted });
    let mut f = ChangeFilter::default();
    // The first reading goes out, "no device" included.
    assert!(f.observe(None));
    assert!(!f.observe(None));
    assert!(f.observe(level(60, false)));
    assert!(!f.observe(level(60, false)));
    assert!(f.observe(level(62, false)));
    assert!(f.observe(level(62, true)));
    assert!(f.observe(None));
    assert!(f.observe(level(62, true)));
    let mut first = ChangeFilter::default();
    assert!(first.observe(level(10, false)));
}

#[test]
fn a_missing_device_is_looked_for_every_minute_and_other_failures_back_off() {
    // No device: one try a minute, however long the streak, so a
    // registration the audio service's restart left dead (which never
    // calls back) cannot leave the thread waiting for good.
    for failures in [0, 1, 2, 9, u32::MAX] {
        assert_eq!(retry_after_ms(BindFailure::NoDevice, failures), 60_000);
    }
    let delays: Vec<u64> = (1..=8)
        .map(|n| retry_after_ms(BindFailure::Other, n))
        .collect();
    assert_eq!(
        delays,
        [2_000, 4_000, 8_000, 16_000, 32_000, 60_000, 60_000, 60_000]
    );
    // A count of 0 is read as the first; a huge one stays at the cap.
    assert_eq!(retry_after_ms(BindFailure::Other, 0), 2_000);
    assert_eq!(retry_after_ms(BindFailure::Other, u32::MAX), 60_000);
    // Never sooner than the first delay nor later than the cap.
    for failure in [BindFailure::NoDevice, BindFailure::Other] {
        for failures in 0..=20 {
            let delay = retry_after_ms(failure, failures);
            assert!((RETRY_FIRST_MS..=RETRY_MAX_MS).contains(&delay));
        }
    }
}

#[test]
fn only_a_failure_other_than_no_device_drops_the_enumerator_at_once() {
    // The audio service restarted: the old enumerator and client may be
    // dead, so they go now and the retry starts from scratch.
    assert!(drops_watch_at_once(BindFailure::Other));
    // No device: the client stays until the retry (which builds it
    // again), so a device that comes meanwhile is noticed at once.
    assert!(!drops_watch_at_once(BindFailure::NoDevice));
}
