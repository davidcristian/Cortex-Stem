use super::*;
use crate::windows::settings_window::native_theme;

#[test]
fn native_theme_forces_light_or_dark_and_follows_windows_for_system() {
    assert_eq!(native_theme(ThemeMode::System), None);
    assert_eq!(native_theme(ThemeMode::Light), Some(tauri::Theme::Light));
    assert_eq!(native_theme(ThemeMode::Dark), Some(tauri::Theme::Dark));
}
