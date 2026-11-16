use pretty_assertions::assert_eq;

use super::*;

fn status(ac: u8, flag: u8, percent: u8, system: u8) -> RawPowerStatus {
    RawPowerStatus {
        ac_line_status: ac,
        battery_flag: flag,
        battery_life_percent: percent,
        system_status_flag: system,
    }
}

fn power(
    has_battery: bool,
    percent: Option<u8>,
    charging: bool,
    on_ac: bool,
    saver: bool,
) -> PowerStatus {
    PowerStatus {
        has_battery,
        percent,
        charging,
        on_ac,
        saver,
    }
}

#[test]
fn the_sdk_constants_are_the_documented_values() {
    // The values the tests below write as literals, as Microsoft's
    // documentation of SYSTEM_POWER_STATUS gives them.
    assert_eq!(AC_ONLINE, 1);
    assert_eq!(CHARGING, 8);
    assert_eq!(NO_BATTERY, 128);
    assert_eq!(FLAG_UNKNOWN, 255);
    assert_eq!(PERCENT_UNKNOWN, 255);
}

#[test]
fn no_system_battery_is_a_desktop_without_a_percent() {
    // What a desktop PC reports: on AC, flag 128, percent unknown.
    assert_eq!(
        from_system(&status(1, 128, 255, 0)),
        power(false, None, false, true, false)
    );
    // A percent next to flag 128 is not believed.
    assert_eq!(
        from_system(&status(1, 128, 100, 0)),
        power(false, None, false, true, false)
    );
    // Flag 128 with other bits set is still no battery, never charging.
    assert_eq!(
        from_system(&status(1, 128 | 8, 255, 0)),
        power(false, None, false, true, false)
    );
    // Battery saver can be on without a battery.
    assert_eq!(
        from_system(&status(1, 128, 255, 1)),
        power(false, None, false, true, true)
    );
    // Everything unknown (a VM, a broken driver): no battery, not on AC.
    assert_eq!(
        from_system(&status(255, 255, 255, 0)),
        power(false, None, false, false, false)
    );
}

#[test]
fn an_unknown_percent_is_none() {
    // A battery (flag "high") whose charge the system does not know.
    assert_eq!(
        from_system(&status(0, 1, 255, 0)),
        power(true, None, false, false, false)
    );
    // Values above 100 are not a charge either.
    for raw in [101, 150, 254] {
        assert_eq!(from_system(&status(0, 2, raw, 0)).percent, None, "{raw}");
    }
    // The ends of the range are kept.
    assert_eq!(from_system(&status(0, 4, 0, 0)).percent, Some(0));
    assert_eq!(from_system(&status(1, 0, 100, 0)).percent, Some(100));
    // An unknown flag with a known charge: a battery, charging unknown.
    assert_eq!(
        from_system(&status(1, 255, 42, 0)),
        power(true, Some(42), false, true, false)
    );
}

#[test]
fn on_battery_discharging() {
    // Flag 1 (high), 2 (low) or 4 (critical), offline.
    assert_eq!(
        from_system(&status(0, 1, 76, 0)),
        power(true, Some(76), false, false, false)
    );
    assert_eq!(
        from_system(&status(0, 2 | 4, 4, 0)),
        power(true, Some(4), false, false, false)
    );
    // A flag of 0 (between "low" and "high" on some systems) is a
    // battery too.
    assert_eq!(
        from_system(&status(0, 0, 50, 0)),
        power(true, Some(50), false, false, false)
    );
    // ACLineStatus unknown counts as not on AC.
    assert_eq!(
        from_system(&status(255, 1, 80, 0)),
        power(true, Some(80), false, false, false)
    );
}

#[test]
fn charging_is_flag_8() {
    assert_eq!(
        from_system(&status(1, 8, 73, 0)),
        power(true, Some(73), true, true, false)
    );
    assert_eq!(
        from_system(&status(1, 1 | 8, 95, 0)),
        power(true, Some(95), true, true, false)
    );
    // Reported as charging off AC (some USB-C chargers): as reported.
    assert_eq!(
        from_system(&status(0, 8, 30, 0)),
        power(true, Some(30), true, false, false)
    );
}

#[test]
fn on_ac_not_charging() {
    // Plugged in at 100 %, or held below full by the charge limit.
    assert_eq!(
        from_system(&status(1, 1, 100, 0)),
        power(true, Some(100), false, true, false)
    );
    assert_eq!(
        from_system(&status(1, 1, 80, 0)),
        power(true, Some(80), false, true, false)
    );
}

#[test]
fn battery_saver_is_system_status_flag_1() {
    assert_eq!(
        from_system(&status(0, 2, 18, 1)),
        power(true, Some(18), false, false, true)
    );
    assert_eq!(
        from_system(&status(1, 8, 18, 1)),
        power(true, Some(18), true, true, true)
    );
    // Off is 0.
    assert!(!from_system(&status(0, 2, 18, 0)).saver);
}

#[test]
fn a_power_saving_status_notification_carries_battery_saver_as_a_dword() {
    let on = 1u32.to_ne_bytes();
    let off = 0u32.to_ne_bytes();
    assert_eq!(notified_saver(GUID_POWER_SAVING_STATUS, &on), Some(true));
    assert_eq!(notified_saver(GUID_POWER_SAVING_STATUS, &off), Some(false));
    // Anything but 0 is on.
    assert_eq!(
        notified_saver(GUID_POWER_SAVING_STATUS, &2u32.to_ne_bytes()),
        Some(true)
    );
    // Data of another size is not a DWORD and says nothing.
    for data in [
        &[][..],
        &[1][..],
        &[1, 0][..],
        &[1, 0, 0, 0, 0, 0, 0, 0][..],
    ] {
        assert_eq!(
            notified_saver(GUID_POWER_SAVING_STATUS, data),
            None,
            "{data:?}"
        );
    }
    // The other registered settings carry no battery-saver state: the
    // AC/DC source's 1 is "on battery", the percent's 1 is 1 %.
    assert_eq!(notified_saver(GUID_ACDC_POWER_SOURCE, &on), None);
    assert_eq!(notified_saver(GUID_BATTERY_PERCENTAGE_REMAINING, &on), None);
}

#[test]
fn a_notified_battery_saver_state_takes_the_place_of_the_flag() {
    let read = power(true, Some(40), false, false, false);
    // Nothing notified yet: the reading as it is.
    assert_eq!(with_notified_saver(read, None), read);
    // A notification wins over a flag that still says the other thing,
    // and nothing else changes.
    assert_eq!(
        with_notified_saver(read, Some(true)),
        power(true, Some(40), false, false, true)
    );
    let flag_on = power(true, Some(40), true, true, true);
    assert_eq!(
        with_notified_saver(flag_on, Some(false)),
        power(true, Some(40), true, true, false)
    );
    // Agreeing values change nothing.
    assert_eq!(with_notified_saver(flag_on, Some(true)), flag_on);
    // Without a battery too (battery saver is reported there as well).
    let desktop = power(false, None, false, true, false);
    assert_eq!(
        with_notified_saver(desktop, Some(true)),
        power(false, None, false, true, true)
    );
}

#[test]
fn the_change_filter_reports_the_first_reading_and_then_only_changes() {
    let mut filter = ChangeFilter::default();
    let a = power(true, Some(76), false, false, false);
    assert!(filter.observe(a));
    assert!(!filter.observe(a));
    let b = power(true, Some(75), false, false, false);
    assert!(filter.observe(b));
    assert!(!filter.observe(b));
    // Back to a reading seen before is a change too.
    assert!(filter.observe(a));
    // The first reading is reported even when it is "nothing known".
    let mut filter = ChangeFilter::default();
    assert!(filter.observe(PowerStatus::default()));
    assert!(!filter.observe(PowerStatus::default()));
}

#[test]
fn the_power_broadcasts_that_re_read_the_status_are_the_contracts_three() {
    assert_eq!(
        power_broadcast_cause(PBT_POWERSETTINGCHANGE),
        Some("PBT_POWERSETTINGCHANGE")
    );
    assert_eq!(
        power_broadcast_cause(PBT_APMPOWERSTATUSCHANGE),
        Some("PBT_APMPOWERSTATUSCHANGE")
    );
    assert_eq!(
        power_broadcast_cause(PBT_APMRESUMEAUTOMATIC),
        Some("PBT_APMRESUMEAUTOMATIC")
    );
    // Going to sleep, and the resume codes that follow the automatic
    // one, re-read nothing; the window procedure default-handles them.
    for code in [
        PBT_APMSUSPEND,
        PBT_APMQUERYSUSPEND,
        PBT_APMRESUMESUSPEND,
        PBT_APMRESUMECRITICAL,
    ] {
        assert_eq!(power_broadcast_cause(code), None, "{code:#x}");
        assert_eq!(power_broadcast_name(code), "WM_POWERBROADCAST");
    }
}

#[test]
fn the_posted_battery_saver_state_survives_the_lparam() {
    for saver in [None, Some(false), Some(true)] {
        assert_eq!(saver_from_lparam(saver_to_lparam(saver)), saver);
    }
    // What every other post carries, and anything unexpected, is none.
    assert_eq!(saver_from_lparam(0), None);
    assert_eq!(saver_from_lparam(3), None);
    assert_eq!(saver_from_lparam(-1), None);
}
