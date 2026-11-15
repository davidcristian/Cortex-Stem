//! The power source, the battery and battery saver.
//!
//! Contract for [`current`]: one `GetSystemPowerStatus` call (it reads the
//! power manager's cached state and does not block), mapped by
//! [`from_system`]:
//! - `has_battery`: the system sees a battery. `BatteryFlag` 128 means *no
//!   system battery*; 255 means *unknown status*, which counts as a battery
//!   only while `BatteryLifePercent` still gives a charge (0 to 100), so a
//!   desktop that knows nothing about batteries never shows the item.
//! - `percent`: `BatteryLifePercent` while it is 0 to 100 and there is a
//!   battery; 255 (unknown) and anything else above 100 give `None`.
//! - `charging`: `BatteryFlag` bit 8, with a battery and a known flag.
//! - `on_ac`: `ACLineStatus` 1 (0 is offline, 255 unknown).
//! - `saver`: `SystemStatusFlag` bit 1, battery saver (energy saver on
//!   Windows 11 24H2) on; reported with or without a battery.
//!
//! The events thread (`events`) reads it again on `WM_POWERBROADCAST`
//! (`PBT_POWERSETTINGCHANGE` for the power settings it registered,
//! `PBT_APMPOWERSTATUSCHANGE`, `PBT_APMRESUMEAUTOMATIC`) and reports a
//! reading only when it differs from the last one it reported
//! ([`ChangeFilter`]).
//!
//! Battery saver has its own source too: a `GUID_POWER_SAVING_STATUS`
//! notification carries the new state as its data ([`notified_saver`]).
//! Once one has arrived, the events thread puts its value in place of
//! `SystemStatusFlag` in every reading ([`with_notified_saver`]).
//! Microsoft documents `PBT_APMPOWERSTATUSCHANGE` for power-source and
//! battery-level changes only, so a flag that is not yet updated when the
//! notification is handled would otherwise be dropped as "unchanged", with
//! nothing known to come later and correct it.

use stem_core::PowerStatus;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemServices::GUID_POWER_SAVING_STATUS;
use windows::Win32::System::WindowsProgramming::{
    AC_LINE_ONLINE, BATTERY_FLAG_CHARGING, BATTERY_FLAG_NO_BATTERY, BATTERY_FLAG_UNKNOWN,
    BATTERY_PERCENTAGE_UNKNOWN,
};
use windows::core::GUID;

use crate::PlatformResult;

/// `SYSTEM_POWER_STATUS::ACLineStatus`: on AC power.
const AC_ONLINE: u8 = byte(AC_LINE_ONLINE);
/// `SYSTEM_POWER_STATUS::BatteryFlag`: charging.
const CHARGING: u8 = byte(BATTERY_FLAG_CHARGING);
/// `SYSTEM_POWER_STATUS::BatteryFlag`: no system battery.
const NO_BATTERY: u8 = byte(BATTERY_FLAG_NO_BATTERY);
/// `SYSTEM_POWER_STATUS::BatteryFlag`: unknown status.
const FLAG_UNKNOWN: u8 = byte(BATTERY_FLAG_UNKNOWN);
/// `SYSTEM_POWER_STATUS::SystemStatusFlag`: battery saver on. The SDK names
/// no constant for it; Microsoft's documentation of the field gives 1.
const SAVER_ON: u8 = 1;

// The percent the SDK calls unknown is above the 0-100 range the mapping
// keeps, so it gives `None` without a test of its own.
const _: () = assert!(BATTERY_PERCENTAGE_UNKNOWN > 100);

/// The SDK's `u32` constant as the `u8` the structure's fields hold; a value
/// that does not fit fails the build.
const fn byte(value: u32) -> u8 {
    assert!(value <= u8::MAX as u32);
    value as u8
}

/// The power status right now.
pub fn current() -> PlatformResult<PowerStatus> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` is a live, writable SYSTEM_POWER_STATUS that the call
    // fills in; nothing else is passed.
    unsafe { GetSystemPowerStatus(&mut status) }?;
    log::trace!(
        "GetSystemPowerStatus: ACLineStatus {}, BatteryFlag {}, BatteryLifePercent {}, \
         SystemStatusFlag {}",
        status.ACLineStatus,
        status.BatteryFlag,
        status.BatteryLifePercent,
        status.SystemStatusFlag
    );
    Ok(from_system(&status))
}

/// What a `SYSTEM_POWER_STATUS` says, by the rules in the module docs.
/// `BatteryLifeTime` and `BatteryFullLifeTime` are not used.
pub fn from_system(status: &SYSTEM_POWER_STATUS) -> PowerStatus {
    let flag = status.BatteryFlag;
    let raw_percent = status.BatteryLifePercent;
    let known_percent = (raw_percent <= 100).then_some(raw_percent);
    let has_battery = if flag == FLAG_UNKNOWN {
        known_percent.is_some()
    } else {
        flag & NO_BATTERY == 0
    };
    PowerStatus {
        has_battery,
        percent: known_percent.filter(|_| has_battery),
        charging: has_battery && flag != FLAG_UNKNOWN && flag & CHARGING != 0,
        on_ac: status.ACLineStatus == AC_ONLINE,
        saver: status.SystemStatusFlag & SAVER_ON != 0,
    }
}

/// The battery-saver state a `PBT_POWERSETTINGCHANGE` notification for
/// `setting` delivers with `data` (its `POWERBROADCAST_SETTING::Data`):
/// for `GUID_POWER_SAVING_STATUS` the data is a `DWORD`, 0 for off and 1
/// for on (any other non-zero value is read as on). `None` for every other
/// setting and for data that is not four bytes long.
pub fn notified_saver(setting: &GUID, data: &[u8]) -> Option<bool> {
    if *setting != GUID_POWER_SAVING_STATUS {
        return None;
    }
    let dword: [u8; 4] = data.try_into().ok()?;
    Some(u32::from_ne_bytes(dword) != 0)
}

/// `status` with `saver` taken from the last `GUID_POWER_SAVING_STATUS`
/// notification, when one has arrived ([`notified_saver`]); the other
/// fields are the reading's.
pub fn with_notified_saver(status: PowerStatus, notified: Option<bool>) -> PowerStatus {
    PowerStatus {
        saver: notified.unwrap_or(status.saver),
        ..status
    }
}

/// The events thread's change filter: a reading is reported when it is the
/// first or differs from the last one reported.
#[derive(Debug, Default)]
pub struct ChangeFilter {
    last: Option<PowerStatus>,
}

impl ChangeFilter {
    /// Whether `status` is to be reported; it becomes the last one if so.
    pub fn observe(&mut self, status: PowerStatus) -> bool {
        if self.last == Some(status) {
            return false;
        }
        self.last = Some(status);
        true
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use windows::Win32::System::SystemServices::{
        GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING,
    };

    use super::*;

    fn status(ac: u8, flag: u8, percent: u8, system: u8) -> SYSTEM_POWER_STATUS {
        SYSTEM_POWER_STATUS {
            ACLineStatus: ac,
            BatteryFlag: flag,
            BatteryLifePercent: percent,
            SystemStatusFlag: system,
            BatteryLifeTime: u32::MAX,
            BatteryFullLifeTime: u32::MAX,
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
        assert_eq!(BATTERY_PERCENTAGE_UNKNOWN, 255);
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
        assert_eq!(notified_saver(&GUID_POWER_SAVING_STATUS, &on), Some(true));
        assert_eq!(notified_saver(&GUID_POWER_SAVING_STATUS, &off), Some(false));
        // Anything but 0 is on.
        assert_eq!(
            notified_saver(&GUID_POWER_SAVING_STATUS, &2u32.to_ne_bytes()),
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
                notified_saver(&GUID_POWER_SAVING_STATUS, data),
                None,
                "{data:?}"
            );
        }
        // The other registered settings carry no battery-saver state: the
        // AC/DC source's 1 is "on battery", the percent's 1 is 1 %.
        assert_eq!(notified_saver(&GUID_ACDC_POWER_SOURCE, &on), None);
        assert_eq!(
            notified_saver(&GUID_BATTERY_PERCENTAGE_REMAINING, &on),
            None
        );
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
    fn this_hosts_status_reads_and_is_consistent() {
        // The real call (it only reads): whatever this machine is, the
        // reading keeps the mapping's invariants.
        let status = current().unwrap();
        if !status.has_battery {
            assert_eq!(status.percent, None);
            assert!(!status.charging);
        }
        assert!(status.percent.is_none_or(|p| p <= 100));
    }
}
