use pretty_assertions::assert_eq;

use super::*;

#[test]
fn display_name_fallback_order() {
    assert_eq!(
        display_name_fallback(Some(r"C:\x\notepad.exe"), "Untitled", "Notepad"),
        "notepad"
    );
    assert_eq!(
        display_name_fallback(None, "  Untitled ", "Notepad"),
        "Untitled"
    );
    assert_eq!(display_name_fallback(None, "", "Notepad"), "Notepad");
    assert_eq!(display_name_fallback(None, "", " "), "Application");
    assert_eq!(
        first_non_empty([None, Some(" "), Some(" a ")]),
        Some("a".into())
    );
    // No fallback ends in `.exe`; a candidate that is nothing but
    // the suffix is skipped.
    assert_eq!(
        display_name_fallback(Some(r"C:\x\notepad.exe.exe"), "", "Notepad"),
        "notepad"
    );
    assert_eq!(
        display_name_fallback(None, "Notepad.exe", "Notepad"),
        "Notepad"
    );
    assert_eq!(
        display_name_fallback(Some(r"C:\x\.exe"), "TASKMGR.EXE", ""),
        "TASKMGR"
    );
    assert_eq!(
        display_name_fallback(Some(".EXE"), ".exe", ".Exe"),
        "Application"
    );
}

#[test]
fn clean_display_name_strips_every_exe_suffix() {
    assert_eq!(clean_display_name("Notepad.exe"), Some("Notepad".into()));
    assert_eq!(clean_display_name("  TASKMGR.EXE "), Some("TASKMGR".into()));
    assert_eq!(clean_display_name("a.exe.Exe"), Some("a".into()));
    assert_eq!(clean_display_name("app.exe "), Some("app".into()));
    assert_eq!(
        clean_display_name("Windows Terminal"),
        Some("Windows Terminal".into())
    );
    // Not a suffix: untouched.
    assert_eq!(clean_display_name("exe"), Some("exe".into()));
    assert_eq!(clean_display_name("Exec"), Some("Exec".into()));
    assert_eq!(clean_display_name("my.exe.txt"), Some("my.exe.txt".into()));
    assert_eq!(clean_display_name("日本語.exe"), Some("日本語".into()));
    assert_eq!(clean_display_name("日本語"), Some("日本語".into()));
    // Nothing left.
    assert_eq!(clean_display_name(".exe"), None);
    assert_eq!(clean_display_name(" .EXE.exe "), None);
    assert_eq!(clean_display_name("   "), None);
    assert_eq!(clean_display_name(""), None);
    assert_eq!(
        first_display_name([None, Some("x.exe.exe"), Some(".exe"), Some("Second")]),
        Some("x".into())
    );
    assert_eq!(
        first_display_name([Some(".exe"), Some(" "), Some("Second.EXE")]),
        Some("Second".into())
    );
    assert_eq!(first_display_name([Some(".exe"), None]), None);
}

#[test]
fn aumid_fallback_names() {
    assert_eq!(
        aumid_fallback_name("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
        "WindowsCalculator"
    );
    assert_eq!(aumid_fallback_name("Chrome"), "Chrome");
    assert_eq!(aumid_fallback_name("Chrome.Profile1"), "Profile1");
    assert_eq!(aumid_fallback_name("!App"), "!App");
    // Never `.exe`, never empty.
    assert_eq!(aumid_fallback_name("Vendor.Tool.exe"), "Tool");
    assert_eq!(aumid_fallback_name(".exe"), "Application");
    assert_eq!(aumid_fallback_name(""), "Application");
}

#[test]
fn version_resource_helpers() {
    assert_eq!(
        version_string_path(0x0409, 0x04B0, "FileDescription"),
        r"\StringFileInfo\040904b0\FileDescription"
    );
    assert_eq!(
        translations(&[0x09, 0x04, 0xB0, 0x04, 0x07, 0x04, 0xE4, 0x04, 0xFF]),
        vec![(0x0409, 0x04B0), (0x0407, 0x04E4)]
    );
    assert_eq!(
        translation_candidates(&[(0x0407, 0x04E4), (0x0409, 0x04B0)]),
        vec![
            (0x0407, 0x04E4),
            (0x0409, 0x04B0),
            (0x0409, 0x04E4),
            (0x0000, 0x04B0)
        ]
    );
    assert_eq!(translation_candidates(&[]).len(), 3);
}

#[test]
fn a_version_string_value_is_read_up_to_its_nul_inside_the_block() {
    let bytes: Vec<u8> = "Notepad\0rest"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    // With and without the NUL in the length, and a shorter length.
    assert_eq!(version_string_value(&bytes, 8), "Notepad");
    assert_eq!(version_string_value(&bytes, 7), "Notepad");
    assert_eq!(version_string_value(&bytes, 3), "Not");
    // A length past the block is cut to it, an odd last byte dropped.
    assert_eq!(version_string_value(&bytes[..5], 100), "No");
    assert_eq!(version_string_value(&[], 4), "");
}
