use windows::Win32::Foundation::LPARAM;
use windows::Win32::System::Power::POWERBROADCAST_SETTING;
use windows::Win32::System::SystemServices::{
    GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING, GUID_POWER_SAVING_STATUS,
};

use super::power_status::{POWER_SETTING_DATA_MAX, POWER_SETTINGS, power_setting_saver};
use super::window::{SETTING_AREA_MAX, setting_is_intl};

/// `s` as UTF-16 with its NUL, one byte into a buffer, so the string is
/// not aligned the way a `u16` is; the string starts at byte 1.
fn unaligned_wide(s: &str) -> Vec<u8> {
    let mut buffer = vec![0u8];
    for unit in s.encode_utf16().chain(std::iter::once(0)) {
        buffer.extend_from_slice(&unit.to_ne_bytes());
    }
    buffer
}

fn area_is_intl(s: &str) -> bool {
    let buffer = unaligned_wide(s);
    // SAFETY: the pointer is a NUL-terminated UTF-16 string inside
    // `buffer`, alive for the call.
    unsafe { setting_is_intl(LPARAM(buffer[1..].as_ptr() as isize)) }
}

#[test]
fn the_intl_area_is_read_out_of_a_setting_change() {
    assert!(area_is_intl("intl"));
    assert!(area_is_intl("Intl"));
    for other in ["", "ImmersiveColorSet", "Policy", "int", "intl2"] {
        assert!(!area_is_intl(other), "{other:?}");
    }
    // A name longer than the bound is not read to its end: no match,
    // whatever follows.
    assert!(!area_is_intl(&"x".repeat(SETTING_AREA_MAX + 8)));
    assert!(!area_is_intl(&"i".repeat(SETTING_AREA_MAX)));
    // An aligned string reads the same.
    let aligned: Vec<u16> = "intl".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: as above, for an aligned buffer.
    assert!(unsafe { setting_is_intl(LPARAM(aligned.as_ptr() as isize)) });
    // SAFETY: NULL is allowed and reads nothing.
    assert!(!unsafe { setting_is_intl(LPARAM(0)) });
}

#[test]
fn the_power_settings_are_the_three_the_contract_names() {
    let names: Vec<&str> = POWER_SETTINGS.iter().map(|(_, name)| *name).collect();
    assert_eq!(
        names,
        [
            "GUID_BATTERY_PERCENTAGE_REMAINING",
            "GUID_ACDC_POWER_SOURCE",
            "GUID_POWER_SAVING_STATUS",
        ]
    );
    assert_eq!(*POWER_SETTINGS[0].0, GUID_BATTERY_PERCENTAGE_REMAINING);
    assert_eq!(*POWER_SETTINGS[1].0, GUID_ACDC_POWER_SOURCE);
    assert_eq!(*POWER_SETTINGS[2].0, GUID_POWER_SAVING_STATUS);
}

/// A `POWERBROADCAST_SETTING` for `setting` with `length` in its
/// `DataLength` and `data` at `Data`, written field by field through the
/// structure's own offsets into a byte buffer, one byte in (so at no
/// particular alignment), at least as large as the structure.
fn power_setting(setting: windows::core::GUID, length: u32, data: &[u8]) -> Vec<u8> {
    let header = core::mem::size_of::<POWERBROADCAST_SETTING>();
    let at = core::mem::offset_of!(POWERBROADCAST_SETTING, Data);
    let mut buffer = vec![0u8; 1 + header.max(at + data.len())];
    let guid_at = 1 + core::mem::offset_of!(POWERBROADCAST_SETTING, PowerSetting);
    let guid = buffer[guid_at..guid_at + core::mem::size_of::<windows::core::GUID>()]
        .as_mut_ptr()
        .cast::<windows::core::GUID>();
    // SAFETY: `guid` points at a GUID's worth of bytes of `buffer`;
    // written unaligned.
    unsafe { guid.write_unaligned(setting) };
    let length_at = 1 + core::mem::offset_of!(POWERBROADCAST_SETTING, DataLength);
    buffer[length_at..length_at + 4].copy_from_slice(&length.to_ne_bytes());
    buffer[1 + at..1 + at + data.len()].copy_from_slice(data);
    buffer
}

fn saver_of(buffer: &[u8]) -> Option<bool> {
    // SAFETY: `buffer` holds a whole POWERBROADCAST_SETTING after its
    // first byte, with at least the data its length field names when
    // that is a DWORD's (`power_setting`), alive for the call; a longer
    // length is cut to POWER_SETTING_DATA_MAX, which the buffer holds.
    unsafe { power_setting_saver(LPARAM(buffer[1..].as_ptr() as isize)) }
}

#[test]
fn the_window_procedure_reads_battery_saver_out_of_the_setting() {
    let on = power_setting(GUID_POWER_SAVING_STATUS, 4, &1u32.to_ne_bytes());
    assert_eq!(saver_of(&on), Some(true));
    let off = power_setting(GUID_POWER_SAVING_STATUS, 4, &0u32.to_ne_bytes());
    assert_eq!(saver_of(&off), Some(false));
    // The other registered settings carry no battery-saver state.
    let ac = power_setting(GUID_ACDC_POWER_SOURCE, 4, &1u32.to_ne_bytes());
    assert_eq!(saver_of(&ac), None);
    let percent = power_setting(GUID_BATTERY_PERCENTAGE_REMAINING, 4, &1u32.to_ne_bytes());
    assert_eq!(saver_of(&percent), None);
    // A length that is not a DWORD's says nothing; a huge one is cut to
    // POWER_SETTING_DATA_MAX before anything is read.
    let short = power_setting(GUID_POWER_SAVING_STATUS, 1, &[1]);
    assert_eq!(saver_of(&short), None);
    let huge = power_setting(
        GUID_POWER_SAVING_STATUS,
        u32::MAX,
        &[1; POWER_SETTING_DATA_MAX],
    );
    assert_eq!(saver_of(&huge), None);
    // NULL is none.
    // SAFETY: NULL is allowed and checked first.
    assert_eq!(unsafe { power_setting_saver(LPARAM(0)) }, None);
}
