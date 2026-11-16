use pretty_assertions::assert_eq;

use super::*;

/// A `REPARSE_DATA_BUFFER` with `tag` and `strings` laid out as an app
/// execution alias's (version 3), as `FSCTL_GET_REPARSE_POINT` fills it.
fn reparse_buffer(tag: u32, strings: &[&str]) -> Vec<u8> {
    let mut data = 3u32.to_le_bytes().to_vec();
    for s in strings {
        for unit in s.encode_utf16().chain([0]) {
            data.extend_from_slice(&unit.to_le_bytes());
        }
    }
    let mut buffer = tag.to_le_bytes().to_vec();
    buffer.extend_from_slice(&u16::try_from(data.len()).unwrap().to_le_bytes());
    buffer.extend_from_slice(&[0, 0]);
    buffer.extend_from_slice(&data);
    buffer
}

const NOTEPAD_TARGET: &str = r"C:\Program Files\WindowsApps\Microsoft.WindowsNotepad_11.2607.14.0_x64__8wekyb3d8bbwe\Notepad\Notepad.exe";

#[test]
fn an_app_execution_alias_names_its_package_app() {
    // The `notepad.exe` alias of Windows 11 build 26300, as `fsutil
    // reparsepoint query` shows it: family, AUMID, target, app type.
    let buffer = reparse_buffer(
        IO_REPARSE_TAG_APPEXECLINK,
        &[
            "Microsoft.WindowsNotepad_8wekyb3d8bbwe",
            "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App",
            NOTEPAD_TARGET,
            "0",
        ],
    );
    let expected = AppExecLink {
        package_family: "Microsoft.WindowsNotepad_8wekyb3d8bbwe".into(),
        app_user_model_id: "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App".into(),
        target: NOTEPAD_TARGET.into(),
    };
    assert_eq!(parse_app_exec_link(&buffer), Some(expected.clone()));
    // The ioctl's buffer is larger than what it filled.
    let mut padded = buffer.clone();
    padded.resize(16 * 1024, 0);
    assert_eq!(parse_app_exec_link(&padded), Some(expected));
    // An older alias without the app type still has the three strings.
    assert!(
        parse_app_exec_link(&reparse_buffer(
            IO_REPARSE_TAG_APPEXECLINK,
            &["F_p", "F_p!App", r"C:\x.exe"]
        ))
        .is_some()
    );
}

#[test]
fn other_reparse_points_and_broken_aliases_name_nothing() {
    let strings = ["F_p", "F_p!App", r"C:\x.exe", "0"];
    // A symbolic link's tag (IO_REPARSE_TAG_SYMLINK).
    assert_eq!(
        parse_app_exec_link(&reparse_buffer(0xA000_000C, &strings)),
        None
    );
    // Cut short of the length the header gives, or of the header.
    let whole = reparse_buffer(IO_REPARSE_TAG_APPEXECLINK, &strings);
    assert_eq!(parse_app_exec_link(&whole[..whole.len() - 2]), None);
    assert_eq!(parse_app_exec_link(&whole[..6]), None);
    assert_eq!(parse_app_exec_link(&[]), None);
    // Too few strings, or a blank AUMID.
    assert_eq!(
        parse_app_exec_link(&reparse_buffer(IO_REPARSE_TAG_APPEXECLINK, &["F_p"])),
        None
    );
    assert_eq!(
        parse_app_exec_link(&reparse_buffer(
            IO_REPARSE_TAG_APPEXECLINK,
            &["F_p", " ", r"C:\x.exe"]
        )),
        None
    );
}

#[test]
fn an_alias_filter_names_its_path_in_any_spelling() {
    let filter = r"C:\Windows\notepad.exe";
    assert!(alias_filter_matches(filter, r"C:\Windows\notepad.exe"));
    assert!(alias_filter_matches(filter, r"c:\windows\NOTEPAD.EXE"));
    assert!(alias_filter_matches(filter, "C:/Windows/notepad.exe"));
    assert!(alias_filter_matches(
        r#" "C:\Windows\notepad.exe" "#,
        filter
    ));
    assert!(!alias_filter_matches(
        filter,
        r"C:\Windows\System32\notepad.exe"
    ));
    assert!(!alias_filter_matches(filter, r"D:\Windows\notepad.exe"));
    assert!(!alias_filter_matches("", ""));
}

#[test]
fn executables_under_the_windows_folder_are_told_apart() {
    for path in [
        r"C:\Windows\notepad.exe",
        r"C:\Windows\System32\notepad.exe",
        r"c:\windows\SysWOW64\NOTEPAD.EXE",
        "D:/Windows/regedit.exe",
        r#""C:\Windows\notepad.exe""#,
    ] {
        assert!(windows_dir_exe(path), "{path}");
    }
    for path in [
        r"C:\Windows",
        r"C:\Windows\",
        r"C:\Windows\notepad.txt",
        r"C:\Program Files\Windows NT\Accessories\wordpad.exe",
        r"C:\Users\me\Windows\x.exe",
        r"\\server\Windows\x.exe",
        "notepad.exe",
        "",
    ] {
        assert!(!windows_dir_exe(path), "{path}");
    }
}
