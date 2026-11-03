//! OS theme facts.
//!
//! Contract for [`current_theme`]:
//! - `accent`: the registry value `HKCU\Software\Microsoft\Windows\DWM\AccentColor`
//!   (ABGR -> `#RRGGBB`); `DwmGetColorizationColor` is not used because it
//!   reports the window-frame colour, which differs from the accent when
//!   "show accent colour on title bars" is off.
//! - `dark`: `HKCU\...\Themes\Personalize\AppsUseLightTheme == 0`.
//! - `high_contrast`: `SystemParametersInfoW(SPI_GETHIGHCONTRAST)`.
//! - `transparency`: `HKCU\...\Themes\Personalize\EnableTransparency`.
//!
//! Missing registry values are not errors: each falls back to
//! `ThemeInfo::default()` (see `rules::theme::theme_from_facts`).
//!
//! Change notifications are delivered by the event thread (`events`), which
//! calls this function again on `WM_SETTINGCHANGE` / `WM_DWMCOLORIZATIONCOLORCHANGED`.

use stem_core::ThemeInfo;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{PCWSTR, w};

use crate::PlatformResult;
use crate::rules::theme::{RawThemeFacts, theme_from_facts};

const DWM_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\DWM");
const PERSONALIZE_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");

pub fn current_theme() -> PlatformResult<ThemeInfo> {
    let facts = RawThemeFacts {
        accent_abgr: read_hkcu_dword(DWM_KEY, w!("AccentColor")),
        apps_use_light_theme: read_hkcu_dword(PERSONALIZE_KEY, w!("AppsUseLightTheme")),
        enable_transparency: read_hkcu_dword(PERSONALIZE_KEY, w!("EnableTransparency")),
        high_contrast: high_contrast_enabled(),
    };
    log::trace!("theme facts: {facts:?}");
    Ok(theme_from_facts(facts))
}

/// A `REG_DWORD` under `HKEY_CURRENT_USER`, or `None` when missing or of
/// another type.
fn read_hkcu_dword(subkey: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut data: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `data` is a 4-byte buffer and `size` says so; RRF_RT_REG_DWORD
    // makes the call fail instead of writing a value of another type; the
    // key/value names are static NUL-terminated literals.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

/// `SPI_GETHIGHCONTRAST`; `None` if the query itself fails.
fn high_contrast_enabled() -> Option<bool> {
    let mut hc = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: `hc` is a valid structure of the size passed in both `cbSize`
    // and `uiParam`; nothing is written to the profile (flags 0).
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some((&raw mut hc).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    match result {
        Ok(()) => Some(hc.dwFlags.contains(HCF_HIGHCONTRASTON)),
        Err(e) => {
            log::debug!("SPI_GETHIGHCONTRAST failed: {e}");
            None
        }
    }
}
