//! The user's regional format, for the clock.
//!
//! The page formats the clock with `Intl`, whose default locale in WebView2
//! is the Windows *display language*; the taskbar's clock follows the
//! *regional format* and its short time format instead. This module reads
//! those two facts so the page can format the clock the way the taskbar
//! does.
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
//! when a `WM_SETTINGCHANGE` names the `intl` area ([`is_intl_area`]), and
//! reports a reading only when it differs from the last one it reported
//! ([`ChangeFilter`]).

use stem_core::ClockFormat;
use windows::Win32::Globalization::{GetLocaleInfoEx, GetUserDefaultLocaleName, LOCALE_SSHORTTIME};
use windows::Win32::System::SystemServices::LOCALE_NAME_MAX_LENGTH;
use windows::core::PCWSTR;

use super::util;

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

/// The BCP 47 name for a Windows locale name: trimmed, without an
/// alternate sort order (`de-DE_phoneb` is `de-DE`); `None` when nothing
/// is left.
pub fn bcp47_name(raw: &str) -> Option<String> {
    let name = raw.split('_').next().unwrap_or_default().trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Whether a Windows time format pattern is a 12-hour one: `Some(true)`
/// for `h`, `Some(false)` for `H`, from the first hour letter outside
/// single quotes (quoted text is literal; two quotes in a row are an empty
/// literal, so the rule holds for them too); `None` without one. In a
/// semicolon-delimited list (`HH:mm;h:mm tt`) that is the first, preferred
/// entry's hour letter.
pub fn hour12_of(pattern: &str) -> Option<bool> {
    let mut quoted = false;
    for c in pattern.chars() {
        match c {
            '\'' => quoted = !quoted,
            'h' if !quoted => return Some(true),
            'H' if !quoted => return Some(false),
            _ => {}
        }
    }
    None
}

/// Whether the area a `WM_SETTINGCHANGE` names (its `lParam` string, as
/// UTF-16 without the NUL) is `intl`, the one Windows sends after a change
/// of the regional format or of one of its formats; ASCII case is ignored.
pub fn is_intl_area(area: &[u16]) -> bool {
    const INTL: [u8; 4] = *b"intl";
    area.len() == INTL.len()
        && area.iter().zip(INTL).all(|(&unit, expected)| {
            u8::try_from(unit).is_ok_and(|b| b.to_ascii_lowercase() == expected)
        })
}

/// The events thread's change filter: a reading is reported when it is the
/// first or differs from the last one reported.
#[derive(Debug, Default)]
pub struct ChangeFilter {
    last: Option<ClockFormat>,
}

impl ChangeFilter {
    /// Whether `format` is to be reported; it becomes the last one if so.
    pub fn observe(&mut self, format: &ClockFormat) -> bool {
        if self.last.as_ref() == Some(format) {
            return false;
        }
        self.last = Some(format.clone());
        true
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn h_is_twelve_hour_and_capital_h_twenty_four() {
        // The en-US and en-GB defaults, and a few other regions'.
        assert_eq!(hour12_of("h:mm tt"), Some(true));
        assert_eq!(hour12_of("HH:mm"), Some(false));
        assert_eq!(hour12_of("H:mm"), Some(false));
        assert_eq!(hour12_of("tt h:mm"), Some(true));
        assert_eq!(hour12_of("hh:mm"), Some(true));
        // A day period first (ko-KR), and a dot between hour and minute.
        assert_eq!(hour12_of("tt hh:mm"), Some(true));
        assert_eq!(hour12_of("H.mm"), Some(false));
    }

    #[test]
    fn quoted_text_is_literal_and_ignored() {
        // A custom 24-hour format with an `h` in its literal text.
        assert_eq!(hour12_of("HH'h'mm"), Some(false));
        assert_eq!(hour12_of("'heure' H:mm"), Some(false));
        assert_eq!(hour12_of("'Hora' h:mm tt"), Some(true));
        // Two quotes in a row are an empty literal: what follows is not quoted.
        assert_eq!(hour12_of("''h:mm"), Some(true));
        assert_eq!(hour12_of("'o''clock' H"), Some(false));
        // An unterminated quote makes the rest literal.
        assert_eq!(hour12_of("'h:mm"), None);
    }

    #[test]
    fn the_first_hour_letter_decides() {
        assert_eq!(hour12_of("HH:mm (h tt)"), Some(false));
        assert_eq!(hour12_of("h:mm (HH)"), Some(true));
    }

    #[test]
    fn a_list_of_formats_takes_the_first_preferred_one() {
        // LOCALE_SSHORTTIME may be a semicolon-delimited list whose first
        // entry is the preferred format.
        assert_eq!(hour12_of("HH:mm;h:mm tt"), Some(false));
        assert_eq!(hour12_of("h:mm tt;HH:mm"), Some(true));
        // A quoted semicolon is literal text, not a separator.
        assert_eq!(hour12_of("'a;b' HH:mm;h:mm tt"), Some(false));
    }

    #[test]
    fn no_hour_letter_is_none() {
        assert_eq!(hour12_of(""), None);
        assert_eq!(hour12_of("mm:ss"), None);
        assert_eq!(hour12_of("tt"), None);
        assert_eq!(hour12_of("'HH:mm'"), None);
    }

    #[test]
    fn locale_names_are_made_bcp_47() {
        assert_eq!(bcp47_name("en-GB").as_deref(), Some("en-GB"));
        assert_eq!(bcp47_name("zh-Hant-TW").as_deref(), Some("zh-Hant-TW"));
        assert_eq!(bcp47_name("sr-Latn-RS").as_deref(), Some("sr-Latn-RS"));
        // An alternate sort order is dropped.
        assert_eq!(bcp47_name("de-DE_phoneb").as_deref(), Some("de-DE"));
        assert_eq!(bcp47_name("es-ES_tradnl").as_deref(), Some("es-ES"));
        assert_eq!(bcp47_name(" en-US ").as_deref(), Some("en-US"));
        // Nothing left: no name.
        assert_eq!(bcp47_name(""), None);
        assert_eq!(bcp47_name("_phoneb"), None);
        assert_eq!(bcp47_name("  "), None);
    }

    #[test]
    fn only_the_intl_area_is_intl() {
        assert!(is_intl_area(&wide("intl")));
        assert!(is_intl_area(&wide("Intl")));
        assert!(is_intl_area(&wide("INTL")));
        for other in [
            "",
            "int",
            "intl ",
            "intlx",
            "ImmersiveColorSet",
            "Policy",
            "Environment",
            "WindowMetrics",
        ] {
            assert!(!is_intl_area(&wide(other)), "{other:?}");
        }
        // A unit outside ASCII whose low byte would read as a letter.
        assert!(!is_intl_area(&[0x0169, 0x006E, 0x0074, 0x006C]));
    }

    #[test]
    fn the_filter_reports_the_first_reading_and_then_only_changes() {
        let mut filter = ChangeFilter::default();
        let unknown = ClockFormat::default();
        assert!(filter.observe(&unknown));
        assert!(!filter.observe(&unknown));
        let british = ClockFormat {
            locale: Some("en-GB".into()),
            hour12: Some(false),
        };
        assert!(filter.observe(&british));
        assert!(!filter.observe(&british.clone()));
        let custom = ClockFormat {
            hour12: Some(true),
            ..british.clone()
        };
        assert!(filter.observe(&custom));
        assert!(filter.observe(&british));
    }

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
