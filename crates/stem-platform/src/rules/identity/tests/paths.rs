use pretty_assertions::assert_eq;

use super::*;

#[test]
fn apps_folder_parsing() {
    assert_eq!(
        parse_apps_folder(r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
        Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App")
    );
    assert_eq!(
        parse_apps_folder("  SHELL:appsfolder/Chrome  "),
        Some("Chrome")
    );
    assert_eq!(parse_apps_folder(r"shell:AppsFolder\"), None);
    assert_eq!(parse_apps_folder("shell:AppsFolder"), None);
    assert_eq!(parse_apps_folder("shell:RecycleBinFolder"), None);
    assert_eq!(parse_apps_folder(r"C:\AppsFolder\x"), None);
    assert_eq!(parse_apps_folder(""), None);
    assert_eq!(
        apps_folder_parsing_name(" Chrome "),
        r"shell:AppsFolder\Chrome"
    );
}

#[test]
fn file_names_and_stems_accept_both_separators() {
    assert_eq!(file_name(r"C:\Program Files\Foo\Foo.exe"), "Foo.exe");
    assert_eq!(file_name("c:/x/y/"), "y");
    assert_eq!(file_name("plain"), "plain");
    assert_eq!(file_stem(r"C:\A\b.EXE"), "b");
    assert_eq!(file_stem("/tmp/.hidden"), ".hidden");
    assert_eq!(file_stem("archive.tar.gz"), "archive.tar");
    assert!(has_extension(r"C:\Users\me\Desktop\App.LNK", "lnk"));
    assert!(!has_extension(r"C:\lnk", "lnk"));
    assert!(!has_extension(".lnk", "lnk"));
    assert!(!has_extension("app.exe", "lnk"));
}

#[test]
fn env_expansion_detection() {
    assert!(needs_env_expansion(r"%ProgramFiles%\Foo\foo.exe"));
    assert!(needs_env_expansion(r"C:\%USERNAME%\x"));
    assert!(!needs_env_expansion(r"C:\Program Files\foo.exe"));
    assert!(!needs_env_expansion("100%"));
    assert!(!needs_env_expansion("50% off, 100% sure"));
    assert!(!needs_env_expansion(""));
}

#[test]
fn an_exe_starts_in_its_own_folder_unless_told_otherwise() {
    let cases = [
        (
            r"C:\Program Files\App\app.exe",
            Some(r"C:\Program Files\App"),
        ),
        (r"C:\app.exe", Some(r"C:\")),
        ("D:/Games/old/game.exe", Some("D:/Games/old")),
        (
            r"\\server\share\tools\tool.exe",
            Some(r"\\server\share\tools"),
        ),
        (
            r#" "C:\Program Files\App\app.exe" "#,
            Some(r"C:\Program Files\App"),
        ),
        ("app.exe", None),
        (r"C:\Apps\", None),
        (r"\app.exe", None),
        ("", None),
    ];
    for (path, expected) in cases {
        assert_eq!(default_working_dir(path), expected, "{path:?}");
    }
}
