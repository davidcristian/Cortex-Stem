//! The power status: the setting notifications of the hidden window,
//! battery saver, and `PowerChanged`.
//!
//! Contract:
//! - Power: the hidden window is registered
//!   (`RegisterPowerSettingNotification`, `DEVICE_NOTIFY_WINDOW_HANDLE`)
//!   for `GUID_BATTERY_PERCENTAGE_REMAINING`, `GUID_ACDC_POWER_SOURCE` and
//!   `GUID_POWER_SAVING_STATUS`; a registration that fails is logged at
//!   `warn` and the others stay (the `PBT_APMPOWERSTATUSCHANGE` broadcast
//!   still arrives). `WM_POWERBROADCAST` with `PBT_POWERSETTINGCHANGE`,
//!   `PBT_APMPOWERSTATUSCHANGE` or `PBT_APMRESUMEAUTOMATIC` re-reads
//!   `power::current` and emits `PowerChanged` only when the reading differs
//!   from the last one emitted (`rules::power::ChangeFilter`); the first
//!   reading is emitted right after the start snapshot. The procedure
//!   answers `TRUE`.
//!   For a `PBT_POWERSETTINGCHANGE` it copies one value out of `lParam` (a
//!   `POWERBROADCAST_SETTING`, valid only during the call; at most
//!   [`POWER_SETTING_DATA_MAX`] bytes of data are looked at): the battery
//!   saver state of a `GUID_POWER_SAVING_STATUS` notification
//!   (`rules::power::notified_saver`), posted with the code. From then on
//!   the last such value takes the place of `SystemStatusFlag` in every reading
//!   (`rules::power::with_notified_saver`): Microsoft documents
//!   `PBT_APMPOWERSTATUSCHANGE` for power-source and battery-level changes
//!   only, so nothing is known to follow a saver toggle that would correct
//!   a `SystemStatusFlag` read too early. Debug lines:
//!   `GUID_POWER_SAVING_STATUS: battery saver on|off` per notification, and
//!   `SystemStatusFlag says battery saver …` when a reading's flag
//!   disagrees with it. Each registration is a guard
//!   ([`PowerNotification`]), unregistered on this thread before the window
//!   is destroyed. Debug line per emission: `power: <reading> (<cause>)`.

use stem_core::PlatformEvent;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM};
use windows::Win32::System::Power::{
    HPOWERNOTIFY, POWERBROADCAST_SETTING, RegisterPowerSettingNotification,
    UnregisterPowerSettingNotification,
};
use windows::Win32::System::SystemServices::{
    GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING, GUID_POWER_SAVING_STATUS,
};
use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_WINDOW_HANDLE;

use super::state::ThreadState;
use crate::rules::power as power_rules;
use crate::windows::power;

/// The most bytes of a `POWERBROADCAST_SETTING`'s data the window procedure
/// looks at. The one value it reads is a `DWORD`; a larger
/// `DataLength` is cut to this, which `rules::power::notified_saver` then
/// rejects as not a `DWORD`.
pub(super) const POWER_SETTING_DATA_MAX: usize = 16;

/// The power settings the hidden window is registered for, with
/// their names for the log.
pub(super) const POWER_SETTINGS: [(&windows::core::GUID, &str); 3] = [
    (
        &GUID_BATTERY_PERCENTAGE_REMAINING,
        "GUID_BATTERY_PERCENTAGE_REMAINING",
    ),
    (&GUID_ACDC_POWER_SOURCE, "GUID_ACDC_POWER_SOURCE"),
    (&GUID_POWER_SAVING_STATUS, "GUID_POWER_SAVING_STATUS"),
];

/// A `RegisterPowerSettingNotification` of the hidden window;
/// unregistered on drop, on the events thread, before the window goes.
pub(super) struct PowerNotification {
    handle: HPOWERNOTIFY,
    name: &'static str,
}

impl PowerNotification {
    /// Register `hwnd` for every setting of [`POWER_SETTINGS`]; the ones that
    /// fail are logged and left out.
    pub(super) fn register_all(hwnd: HWND) -> Vec<Self> {
        let mut registered = Vec::with_capacity(POWER_SETTINGS.len());
        for (guid, name) in POWER_SETTINGS {
            // SAFETY: `hwnd` is a live window owned by this thread, passed as
            // the recipient handle DEVICE_NOTIFY_WINDOW_HANDLE asks for; the
            // GUID is a static the call only reads.
            let result = unsafe {
                RegisterPowerSettingNotification(HANDLE(hwnd.0), guid, DEVICE_NOTIFY_WINDOW_HANDLE)
            };
            match result {
                Ok(handle) => registered.push(Self { handle, name }),
                Err(e) => log::warn!(
                    "RegisterPowerSettingNotification({name}) failed: {e}; the power status \
                     broadcast still updates the battery"
                ),
            }
        }
        log::debug!(
            "power settings registered: {}",
            registered
                .iter()
                .map(|n| n.name)
                .collect::<Vec<_>>()
                .join(", ")
        );
        registered
    }
}

impl Drop for PowerNotification {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful
        // RegisterPowerSettingNotification and is unregistered once.
        if let Err(e) = unsafe { UnregisterPowerSettingNotification(self.handle) } {
            log::debug!(
                "UnregisterPowerSettingNotification({}) failed: {e}",
                self.name
            );
        }
    }
}

impl ThreadState {
    /// Read the power status, with battery saver as the last
    /// `GUID_POWER_SAVING_STATUS` notification said once one has arrived,
    /// and emit `PowerChanged` when it is the first reading or differs from
    /// the last one emitted; `cause` names what asked, for the log. A failed
    /// read is logged and emits nothing.
    pub(super) fn refresh_power(&mut self, cause: &str) {
        match power::current() {
            Ok(read) => {
                let status = power_rules::with_notified_saver(read, self.notified_saver);
                if status.saver != read.saver {
                    log::debug!(
                        "SystemStatusFlag says battery saver {}, the last \
                         GUID_POWER_SAVING_STATUS notification {}; using the notification",
                        if read.saver { "on" } else { "off" },
                        if status.saver { "on" } else { "off" }
                    );
                }
                if self.power.observe(status) {
                    log::debug!("power: {status:?} ({cause})");
                    self.emit(PlatformEvent::PowerChanged(status));
                } else {
                    log::trace!("power unchanged ({cause})");
                }
            }
            Err(e) => log::warn!("GetSystemPowerStatus failed ({cause}): {e}"),
        }
    }
}

/// The battery-saver state a `PBT_POWERSETTINGCHANGE` carries:
/// `rules::power::notified_saver` of the `POWERBROADCAST_SETTING` behind
/// `lparam`, copied out while the window procedure runs; `None` for a NULL
/// `lparam` and for every other setting. Reads the header and at most
/// [`POWER_SETTING_DATA_MAX`] bytes of the data, without assuming alignment.
///
/// # Safety
///
/// `lparam` is NULL or points at a readable `POWERBROADCAST_SETTING` whose
/// `DataLength` bytes of data follow at `Data`, as the `lParam` of a
/// `WM_POWERBROADCAST` with `PBT_POWERSETTINGCHANGE` does while it is being
/// handled.
pub(super) unsafe fn power_setting_saver(lparam: LPARAM) -> Option<bool> {
    let base = lparam.0 as *const u8;
    if base.is_null() {
        return None;
    }
    // The fields' addresses, by the structure's own offsets; no reference to
    // the structure is made, and nothing assumes its alignment.
    let field = |offset: usize| base.wrapping_add(offset);
    let guid = field(core::mem::offset_of!(POWERBROADCAST_SETTING, PowerSetting));
    let length = field(core::mem::offset_of!(POWERBROADCAST_SETTING, DataLength));
    let data = field(core::mem::offset_of!(POWERBROADCAST_SETTING, Data));
    // SAFETY: the caller guarantees a readable header, so the GUID's bytes
    // are readable; read unaligned.
    let guid = unsafe { guid.cast::<windows::core::GUID>().read_unaligned() };
    // SAFETY: as above, for the length field.
    let length = unsafe { length.cast::<u32>().read_unaligned() };
    let length = (length as usize).min(POWER_SETTING_DATA_MAX);
    // SAFETY: the caller guarantees `DataLength` readable bytes at `Data`;
    // `length` is no more than that, the bytes are `u8` (no alignment), and
    // the slice is dropped before this function returns.
    let data = unsafe { core::slice::from_raw_parts(data, length) };
    power_rules::notified_saver(guid.to_u128(), data)
}
