use pretty_assertions::assert_eq;
use stem_core::IconKey;

use super::*;

#[test]
fn icon_keys_are_namespaced() {
    assert_eq!(exe_icon_key(r"C:\a.exe").0, r"exe:C:\a.exe");
    assert_eq!(file_icon_key(r"C:\a.lnk").0, r"file:C:\a.lnk");
    assert_eq!(aumid_icon_key(" X!App ").0, "aumid:X!App");
    assert_eq!(hwnd_icon_key(66, 4242).0, "hwnd:66:4242");
}

#[test]
fn hwnd_icon_values_parse_with_and_without_a_pid() {
    assert_eq!(parse_hwnd_icon_value("66:4242"), Some(66));
    assert_eq!(parse_hwnd_icon_value("66"), Some(66));
    assert_eq!(parse_hwnd_icon_value("66:"), Some(66));
    assert_eq!(parse_hwnd_icon_value(""), None);
    assert_eq!(parse_hwnd_icon_value(":4242"), None);
    assert_eq!(parse_hwnd_icon_value("x:1"), None);
}

#[test]
fn a_recycled_handle_of_another_process_is_not_the_keys_window() {
    assert!(hwnd_icon_owner_matches("66:4242", 4242));
    assert!(!hwnd_icon_owner_matches("66:4242", 777));
    // GetWindowThreadProcessId failed: no process, no match.
    assert!(!hwnd_icon_owner_matches("66:4242", 0));
    // An older key without a pid cannot tell.
    assert!(hwnd_icon_owner_matches("66", 777));
    assert!(hwnd_icon_owner_matches("66:", 777));
    assert!(!hwnd_icon_owner_matches("66:x", 777));
}

#[test]
fn only_hwnd_keys_are_bound_to_a_window() {
    assert!(is_window_icon_key(&hwnd_icon_key(66, 4242)));
    assert!(!is_window_icon_key(&exe_icon_key(r"C:\a.exe")));
    assert!(!is_window_icon_key(&aumid_icon_key("X!App")));
    assert!(!is_window_icon_key(&file_icon_key(r"C:\a.lnk")));
    assert!(!is_window_icon_key(&IconKey("mock:exe:c:/a.exe".into())));
    // An older key without a pid is one too; the namespace is compared
    // whole, and a key without one is not.
    assert!(is_window_icon_key(&IconKey("hwnd:66".into())));
    assert!(!is_window_icon_key(&IconKey("hwndx:1".into())));
    assert!(!is_window_icon_key(&IconKey("hwnd".into())));
    assert!(!is_window_icon_key(&IconKey("xhwnd:1".into())));
}
