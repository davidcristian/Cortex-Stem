//! Shell actions behind the fixed dock items: the
//! Settings pages and what a cut-short key chord leaves down.
//!
//! `windows::shell` opens the pages and sends the chords with `SendInput`;
//! the page URIs and [`held_after`], which says which keys a short count of
//! inserted events left down, are here so they are unit-tested on every
//! host. [`held_after`] works on any key type (the platform passes
//! `VIRTUAL_KEY`s).

/// The Settings app's *Date & time* page.
pub const DATE_TIME_SETTINGS: &str = "ms-settings:dateandtime";
/// The Settings app's power page: *Power & battery* (or *Power* without a
/// battery) on Windows 11, *Power & sleep* on Windows 10.
pub const POWER_SETTINGS: &str = "ms-settings:powersleep";

/// The keys of `keys` that are still down when only the first `sent` events
/// of its chord (every key down in order, then every key up in reverse) were
/// inserted: the ones pressed and not yet released, in press order.
pub fn held_after<K>(keys: &[K], sent: usize) -> &[K] {
    let pressed = sent.min(keys.len());
    let released = sent.saturating_sub(keys.len()).min(pressed);
    &keys[..pressed - released]
}

#[cfg(test)]
mod tests {
    use super::*;

    // The `VK_*` values of winuser.h, as plain key codes.
    const VK_TAB: u16 = 0x09;
    const VK_A: u16 = 0x41;
    const VK_N: u16 = 0x4E;
    const VK_LWIN: u16 = 0x5B;

    #[test]
    fn the_settings_pages_are_the_documented_ms_settings_uris() {
        // Microsoft's list of the `ms-settings:` scheme: Time and language >
        // Date & time, and System > Power & sleep.
        assert_eq!(DATE_TIME_SETTINGS, "ms-settings:dateandtime");
        assert_eq!(POWER_SETTINGS, "ms-settings:powersleep");
    }

    #[test]
    fn a_short_chord_releases_exactly_the_keys_it_left_down() {
        let chord = [VK_LWIN, VK_A];
        // Events: Win down, A down, A up, Win up.
        assert_eq!(held_after(&chord, 0), &[] as &[u16]);
        assert_eq!(held_after(&chord, 1), &[VK_LWIN]);
        assert_eq!(held_after(&chord, 2), &[VK_LWIN, VK_A]);
        assert_eq!(held_after(&chord, 3), &[VK_LWIN]);
        assert_eq!(held_after(&chord, 4), &[] as &[u16]);
        // A count past the chord's length (never returned) holds nothing.
        assert_eq!(held_after(&chord, 9), &[] as &[u16]);

        // The Start menu's chord: Win down, Win up.
        assert_eq!(held_after(&[VK_LWIN], 0), &[] as &[u16]);
        assert_eq!(held_after(&[VK_LWIN], 1), &[VK_LWIN]);
        assert_eq!(held_after(&[VK_LWIN], 2), &[] as &[u16]);

        // Three keys: every prefix of the six events.
        let three = [VK_LWIN, VK_N, VK_TAB];
        let held: Vec<&[u16]> = (0..=6).map(|sent| held_after(&three, sent)).collect();
        assert_eq!(
            held,
            [
                &[][..],
                &[VK_LWIN][..],
                &[VK_LWIN, VK_N][..],
                &[VK_LWIN, VK_N, VK_TAB][..],
                &[VK_LWIN, VK_N][..],
                &[VK_LWIN][..],
                &[][..],
            ]
        );
    }
}
