//! The user's regional format, for the clock.
//!
//! The page formats the clock with `Intl`, whose default locale in WebView2
//! is the Windows *display language*; the taskbar's clock follows the
//! *regional format* and its short time format instead. This module reads
//! those two facts so the page can format the clock the way the taskbar
//! does. The parsing and the change filter are `rules::locale` (pure,
//! unit-tested on every host); this module makes the two calls.
//!
//! Contract for [`current`]:
//! - `locale`: `GetUserDefaultLocaleName` (the regional format's name, into
//!   a buffer of `LOCALE_NAME_MAX_LENGTH` UTF-16 units), made a BCP 47 name
//!   by [`bcp47_name`]: Windows appends an alternate sort order with an
//!   underscore (`de-DE_phoneb`, `es-ES_tradnl`), which `Intl` rejects, so
//!   that suffix is dropped. A failed call or an empty name gives `None`.
//! - `hour12`: the hour letter of the user's short time format,
//!   `GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_SSHORTTIME)` (user
//!   overrides included, as the Region settings show it; read into a buffer
//!   of [`SHORT_TIME_MAX`] units), by [`hour12_of`]: `H` is a 24-hour clock
//!   (`false`), `h` a 12-hour one (`true`); text inside single quotes is
//!   literal and ignored; the first hour letter decides; a pattern without
//!   one, or a failed call, gives `None`. Microsoft documents that the value
//!   may be a semicolon-delimited list whose first entry is the preferred
//!   format; the first hour letter is that entry's.
//!
//! Both calls read the user's locale data in this process and do not block.
//! A failure is a `debug` line and that field is `None`; the reading itself
//! never fails.
//!
//! The events thread (`events`) reads it after the start snapshot and again
//! when a `WM_SETTINGCHANGE` names the `intl` area
//! (`rules::locale::is_intl_area`), and reports a reading only when it
//! differs from the last one it reported (`rules::locale::ChangeFilter`).

use stem_core::ClockFormat;
use windows::Win32::Globalization::{GetLocaleInfoEx, GetUserDefaultLocaleName, LOCALE_SSHORTTIME};
use windows::Win32::System::SystemServices::LOCALE_NAME_MAX_LENGTH;
use windows::core::PCWSTR;

use super::util;
use crate::rules::locale::{bcp47_name, hour12_of};

/// The most UTF-16 units of the short time format read, its NUL included.
/// A chosen bound, not a documented one: Microsoft gives no maximum for
/// `LOCALE_SSHORTTIME`, and real patterns are a dozen units or so. A
/// longer value fails the call (`ERROR_INSUFFICIENT_BUFFER`) and gives no
/// hour cycle.
pub const SHORT_TIME_MAX: usize = 128;

/// The user's regional format right now, by the rules in the module docs.
pub fn current() -> ClockFormat {
    ClockFormat {
        locale: user_locale_name().as_deref().and_then(bcp47_name),
        hour12: short_time_format().as_deref().and_then(hour12_of),
    }
}

/// `GetUserDefaultLocaleName`, as Windows writes it; `None` when it fails.
fn user_locale_name() -> Option<String> {
    let mut buf = [0u16; LOCALE_NAME_MAX_LENGTH as usize];
    // SAFETY: `buf` is a live, writable buffer; the call is told its length
    // and writes at most that many units, the NUL included.
    let written = unsafe { GetUserDefaultLocaleName(&mut buf) };
    let Ok(written @ 1..) = usize::try_from(written) else {
        log::debug!(
            "GetUserDefaultLocaleName failed: {}",
            windows::core::Error::from_thread()
        );
        return None;
    };
    Some(util::from_wide(&buf[..written.min(buf.len())]))
}

/// The user's short time format (`LOCALE_SSHORTTIME`, such as `h:mm tt` or
/// `HH:mm`); `None` when the call fails.
fn short_time_format() -> Option<String> {
    let mut buf = [0u16; SHORT_TIME_MAX];
    // SAFETY: a NULL name is LOCALE_NAME_USER_DEFAULT (the SDK defines it as
    // NULL); `buf` is a live, writable buffer whose length the call is
    // told, and it writes at most that many units, the NUL included.
    let written = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_SSHORTTIME, Some(&mut buf)) };
    let Ok(written @ 1..) = usize::try_from(written) else {
        log::debug!(
            "GetLocaleInfoEx(LOCALE_SSHORTTIME) failed: {}",
            windows::core::Error::from_thread()
        );
        return None;
    };
    Some(util::from_wide(&buf[..written.min(buf.len())]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_hosts_format_reads() {
        // The real calls (they only read): every Windows user has a
        // regional format whose short time shows the hour.
        let format = current();
        let locale = format.locale.as_deref().expect("a regional format");
        assert!(!locale.is_empty() && !locale.contains('_'), "{format:?}");
        assert!(format.hour12.is_some(), "{format:?}");
    }
}
