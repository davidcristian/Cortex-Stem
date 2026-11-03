//! Theme value conversions.
//!
//! ABGR `DWORD` accent colour → `#RRGGBB`, registry DWORD → bool, etc. Pure
//! and tested here.

use stem_core::ThemeInfo;

/// `HKCU\Software\Microsoft\Windows\DWM\AccentColor` stores `0xAABBGGRR`.
/// The alpha byte is ignored; the result is upper-case `#RRGGBB`.
pub fn abgr_to_hex(abgr: u32) -> String {
    let r = abgr & 0xFF;
    let g = (abgr >> 8) & 0xFF;
    let b = (abgr >> 16) & 0xFF;
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Registry booleans are DWORDs; anything non-zero is `true`.
pub fn dword_to_bool(value: u32) -> bool {
    value != 0
}

/// `AppsUseLightTheme == 0` means dark mode.
pub fn dark_from_apps_use_light_theme(value: u32) -> bool {
    !dword_to_bool(value)
}

/// Raw facts as read from the OS; `None` means the value was missing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RawThemeFacts {
    pub accent_abgr: Option<u32>,
    pub apps_use_light_theme: Option<u32>,
    pub enable_transparency: Option<u32>,
    pub high_contrast: Option<bool>,
}

/// Fold the raw facts into a [`ThemeInfo`], keeping the defaults for
/// anything that could not be read.
pub fn theme_from_facts(facts: RawThemeFacts) -> ThemeInfo {
    let defaults = ThemeInfo::default();
    ThemeInfo {
        accent: facts.accent_abgr.map_or(defaults.accent, abgr_to_hex),
        dark: facts
            .apps_use_light_theme
            .map_or(defaults.dark, dark_from_apps_use_light_theme),
        high_contrast: facts.high_contrast.unwrap_or(defaults.high_contrast),
        transparency: facts
            .enable_transparency
            .map_or(defaults.transparency, dword_to_bool),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn abgr_to_hex_swaps_channels_and_drops_alpha() {
        // Windows default accent (#0078D4) as stored: AA=FF BB=D4 GG=78 RR=00.
        assert_eq!(abgr_to_hex(0xFFD4_7800), "#0078D4");
        assert_eq!(abgr_to_hex(0x0000_00FF), "#FF0000");
        assert_eq!(abgr_to_hex(0xC4FF_0000), "#0000FF");
        assert_eq!(abgr_to_hex(0), "#000000");
        assert_eq!(abgr_to_hex(u32::MAX), "#FFFFFF");
    }

    #[test]
    fn dword_booleans() {
        assert!(!dword_to_bool(0));
        assert!(dword_to_bool(1));
        assert!(dword_to_bool(0xFFFF));
        assert!(dark_from_apps_use_light_theme(0));
        assert!(!dark_from_apps_use_light_theme(1));
    }

    #[test]
    fn facts_fold_into_theme_with_defaults_for_missing_values() {
        assert_eq!(
            theme_from_facts(RawThemeFacts::default()),
            ThemeInfo::default()
        );
        let t = theme_from_facts(RawThemeFacts {
            accent_abgr: Some(0xFF00_FF00),
            apps_use_light_theme: Some(1),
            enable_transparency: Some(0),
            high_contrast: Some(true),
        });
        assert_eq!(
            t,
            ThemeInfo {
                accent: "#00FF00".into(),
                dark: false,
                high_contrast: true,
                transparency: false,
            }
        );
    }
}
