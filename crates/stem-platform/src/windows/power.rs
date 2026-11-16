//! The power source, the battery and battery saver.
//!
//! Contract for [`current`]: one `GetSystemPowerStatus` call (it reads the
//! power manager's cached state and does not block), mapped by
//! `rules::power::from_system`, whose module docs give the rules. What a
//! reading and a `GUID_POWER_SAVING_STATUS` notification say, which
//! `WM_POWERBROADCAST` codes re-read the status, and the change filter the
//! events thread (`events`) keeps, are all `rules::power` (pure,
//! unit-tested on every host); this module makes the call and proves the
//! SDK values that module mirrors.

use stem_core::PowerStatus;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemServices::{
    GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING, GUID_POWER_SAVING_STATUS,
};
use windows::Win32::System::WindowsProgramming::{
    AC_LINE_ONLINE, BATTERY_FLAG_CHARGING, BATTERY_FLAG_NO_BATTERY, BATTERY_FLAG_UNKNOWN,
    BATTERY_PERCENTAGE_UNKNOWN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    PBT_APMPOWERSTATUSCHANGE, PBT_APMQUERYSUSPEND, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL,
    PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PBT_POWERSETTINGCHANGE,
};

use crate::PlatformResult;
use crate::rules::power::{self as rules, RawPowerStatus};

// The host-independent values in `rules::power` mirror the SDK; prove them
// against it here, where the `windows` crate is available.
const _: () = {
    assert!(rules::AC_ONLINE as u32 == AC_LINE_ONLINE);
    assert!(rules::CHARGING as u32 == BATTERY_FLAG_CHARGING);
    assert!(rules::NO_BATTERY as u32 == BATTERY_FLAG_NO_BATTERY);
    assert!(rules::FLAG_UNKNOWN as u32 == BATTERY_FLAG_UNKNOWN);
    assert!(rules::PERCENT_UNKNOWN as u32 == BATTERY_PERCENTAGE_UNKNOWN);
    assert!(rules::GUID_POWER_SAVING_STATUS == GUID_POWER_SAVING_STATUS.to_u128());
    assert!(rules::GUID_ACDC_POWER_SOURCE == GUID_ACDC_POWER_SOURCE.to_u128());
    assert!(
        rules::GUID_BATTERY_PERCENTAGE_REMAINING == GUID_BATTERY_PERCENTAGE_REMAINING.to_u128()
    );
    assert!(rules::PBT_APMQUERYSUSPEND == PBT_APMQUERYSUSPEND);
    assert!(rules::PBT_APMSUSPEND == PBT_APMSUSPEND);
    assert!(rules::PBT_APMRESUMECRITICAL == PBT_APMRESUMECRITICAL);
    assert!(rules::PBT_APMRESUMESUSPEND == PBT_APMRESUMESUSPEND);
    assert!(rules::PBT_APMPOWERSTATUSCHANGE == PBT_APMPOWERSTATUSCHANGE);
    assert!(rules::PBT_APMRESUMEAUTOMATIC == PBT_APMRESUMEAUTOMATIC);
    assert!(rules::PBT_POWERSETTINGCHANGE == PBT_POWERSETTINGCHANGE);
};

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
    Ok(rules::from_system(&RawPowerStatus {
        ac_line_status: status.ACLineStatus,
        battery_flag: status.BatteryFlag,
        battery_life_percent: status.BatteryLifePercent,
        system_status_flag: status.SystemStatusFlag,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
