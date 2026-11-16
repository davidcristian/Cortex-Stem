use pretty_assertions::assert_eq;

use super::*;

const TERMINAL_FULL_NAME: &str = "Microsoft.WindowsTerminal_1.24.12741.0_x64__8wekyb3d8bbwe";

#[test]
fn packaged_exes_are_found_under_windows_apps() {
    assert_eq!(
        packaged_exe(
            r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.24.12741.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe"
        ),
        Some(PackagedExe {
            package_full_name: TERMINAL_FULL_NAME,
            relative_path: "WindowsTerminal.exe",
        })
    );
    // Lower-cased, forward slashes (the form of an `exe:` id), a nested
    // executable, another drive, and surrounding whitespace.
    assert_eq!(
        packaged_exe(
            " c:/program files/windowsapps/claude_4.4212.0.0_x64__pzs8sxrjxfjjc/app/Claude.exe "
        ),
        Some(PackagedExe {
            package_full_name: "claude_4.4212.0.0_x64__pzs8sxrjxfjjc",
            relative_path: "app/Claude.exe",
        })
    );
    assert_eq!(
        packaged_exe(r"D:\WindowsApps\Vendor.App_1.0.0.0_neutral_~_abcdefghijklm\bin\\app.exe\"),
        Some(PackagedExe {
            package_full_name: "Vendor.App_1.0.0.0_neutral_~_abcdefghijklm",
            relative_path: r"bin\\app.exe",
        })
    );
    // The first `WindowsApps` followed by a full name wins.
    assert_eq!(
        packaged_exe(r"C:\WindowsApps\x\WindowsApps\A_1.2.3.4_x86__p\a.exe")
            .map(|p| p.package_full_name),
        Some("A_1.2.3.4_x86__p")
    );
}

#[test]
fn other_paths_are_not_packaged_exes() {
    for path in [
        r"C:\Program Files\Mozilla Firefox\firefox.exe",
        // App execution aliases: a file, or a family-name directory.
        r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\wt.exe",
        r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\Microsoft.WindowsTerminal_8wekyb3d8bbwe\wt.exe",
        // The package directory itself, with or without a separator.
        r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.24.12741.0_x64__8wekyb3d8bbwe",
        r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.24.12741.0_x64__8wekyb3d8bbwe\",
        // Housekeeping directories and the mock's demo path.
        r"C:\Program Files\WindowsApps\Deleted\x.exe",
        r"C:\Program Files\WindowsApps\Terminal\WindowsTerminal.exe",
        r"C:\Program Files\NotWindowsApps\A_1.2.3.4_x86__p\a.exe",
        "",
    ] {
        assert_eq!(packaged_exe(path), None, "{path}");
    }
}

#[test]
fn package_full_name_shape() {
    assert!(is_package_full_name(TERMINAL_FULL_NAME));
    assert!(is_package_full_name(
        "Microsoft.WindowsTerminal_1.24.12741.0_neutral_~_8wekyb3d8bbwe"
    ));
    assert!(is_package_full_name(
        "Microsoft.Lang_1.0.0.0_neutral_split.scale-125_8wekyb3d8bbwe"
    ));
    for name in [
        // The family name: two fields.
        "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
        // Version not four numbers, or out of range.
        "A_1.2.3_x64__p",
        "A_1.2.3.x_x64__p",
        "A_1.2.3.70000_x64__p",
        // Missing name, architecture or publisher id; one field too many.
        "_1.2.3.4_x64__p",
        "A_1.2.3.4___p",
        "A_1.2.3.4_x64__",
        "A_1.2.3.4_x64__p_q",
        "A_1.2.3.4_x-64__p",
        "Terminal",
        "",
    ] {
        assert!(!is_package_full_name(name), "{name}");
    }
}

#[test]
fn package_app_entries_become_full_aumids() {
    let family = "Microsoft.WindowsTerminal_8wekyb3d8bbwe";
    assert_eq!(
        package_app_aumid(family, "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"),
        Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into())
    );
    assert_eq!(
        package_app_aumid(family, " App "),
        Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into())
    );
    assert_eq!(package_app_aumid(family, "  "), None);
}

/// A buffer laid out like `GetPackageApplicationIds` fills it: a table of
/// pointers followed by the strings they point at.
fn string_table(base: usize, strings: &[&str]) -> (Vec<u16>, Vec<usize>) {
    // Leave room for the table itself, as the API does.
    let mut buffer = vec![0u16; strings.len() * 4];
    let mut addresses = Vec::new();
    for s in strings {
        addresses.push(base + buffer.len() * 2);
        buffer.extend(s.encode_utf16());
        buffer.push(0);
    }
    (buffer, addresses)
}

#[test]
fn strings_are_read_through_offsets_into_the_buffer() {
    let base = 0x10_0000;
    let (buffer, addresses) = string_table(base, &["A_p!App", "A_p!Sécond"]);
    assert_eq!(
        strings_at(&buffer, base, &addresses),
        vec!["A_p!App".to_owned(), "A_p!Sécond".to_owned()]
    );
    // Outside the buffer, before it, between two units, or with no NUL
    // before the end: skipped, never read.
    let unterminated = vec![0x41u16, 0x42];
    let bad = [
        base - 2,
        base + buffer.len() * 2,
        addresses[0] + 1,
        usize::MAX,
    ];
    assert!(strings_at(&buffer, base, &bad).is_empty());
    assert!(strings_at(&unterminated, base, &[base]).is_empty());
    // An empty string is skipped; a table with no entries reads nothing.
    assert!(strings_at(&[0, 0], base, &[base]).is_empty());
    assert!(strings_at(&buffer, base, &[]).is_empty());
}

#[test]
fn relative_paths_compare_without_case_or_separator_style() {
    assert!(same_relative_path(r"app\Claude.exe", "APP/claude.EXE"));
    assert!(same_relative_path(
        r"\WindowsTerminal.exe",
        "windowsterminal.exe/"
    ));
    assert!(!same_relative_path(r"amd64\cdb.exe", r"x86\cdb.exe"));
    assert!(!same_relative_path("cdb.exe", r"amd64\cdb.exe"));
}

fn aumids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| format!("F_p!{id}")).collect()
}

fn executables(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(id, exe)| (format!("F_p!{id}"), (*exe).to_owned()))
        .collect()
}

#[test]
fn the_only_application_is_chosen_without_asking_anything() {
    let apps = aumids(&["App"]);
    let chosen = choose_package_app(&apps, &[], "anything.exe", |_| {
        panic!("the shell is not asked about a single application")
    });
    assert_eq!(chosen, Some("F_p!App"));
    assert_eq!(choose_package_app(&[], &[], "x.exe", |_| true), None);
}

#[test]
fn several_applications_are_told_apart_by_executable() {
    // The Store: one application per executable.
    let apps = aumids(&["App", "StoreDesktopExtension", "StoreCLI"]);
    let exes = executables(&[
        ("App", "WinStore.App.exe"),
        ("StoreDesktopExtension", "StoreDesktopExtension.exe"),
        ("StoreCLI", "store.exe"),
    ]);
    let chosen = choose_package_app(&apps, &exes, "winstore.app.exe", |_| {
        panic!("an executable match decides alone")
    });
    assert_eq!(chosen, Some("F_p!App"));
    // A nested executable must match its whole relative path.
    let apps = aumids(&["CdbX64", "CdbX86", "Shell"]);
    let exes = executables(&[
        ("CdbX64", r"amd64\cdb.exe"),
        ("CdbX86", r"x86\cdb.exe"),
        ("Shell", "DbgX.Shell.exe"),
    ]);
    assert_eq!(
        choose_package_app(&apps, &exes, "x86/cdb.exe", |_| true),
        Some("F_p!CdbX86")
    );
}

#[test]
fn the_shell_breaks_a_tie_or_the_choice_is_left_open() {
    // Two applications share the executable (Teams); the shell lists one.
    let apps = aumids(&["RemoteModule", "Main", "Update"]);
    let exes = executables(&[
        ("RemoteModule", "ms-teams.exe"),
        ("Main", "ms-teams.exe"),
        ("Update", "ms-teamsupdate.exe"),
    ]);
    let mut asked = Vec::new();
    let chosen = choose_package_app(&apps, &exes, "ms-teams.exe", |a| {
        asked.push(a.to_owned());
        a.ends_with("!Main")
    });
    assert_eq!(chosen, Some("F_p!Main"));
    // Only the two candidates were looked up, not the updater.
    assert_eq!(asked, vec!["F_p!RemoteModule", "F_p!Main"]);
    // The shell lists both: ambiguous.
    assert_eq!(
        choose_package_app(&apps, &exes, "ms-teams.exe", |_| true),
        None
    );

    // No application runs this executable (a helper process, or the
    // manifest could not be read): the one the shell lists, if alone.
    let apps = aumids(&["Spotify", "SpotifyCli", "Widget"]);
    let exes = executables(&[
        ("Spotify", "SpotifyMigrator.exe"),
        ("SpotifyCli", "spotify_cli.exe"),
        ("Widget", "SpotifyXboxGamebarWebView.exe"),
    ]);
    assert_eq!(
        choose_package_app(&apps, &exes, "Spotify.exe", |a| a.ends_with("!Spotify")),
        Some("F_p!Spotify")
    );
    assert_eq!(
        choose_package_app(&apps, &[], "Spotify.exe", |a| a.ends_with("!Spotify")),
        Some("F_p!Spotify")
    );
    assert_eq!(
        choose_package_app(&apps, &exes, "Spotify.exe", |_| true),
        None
    );
    assert_eq!(
        choose_package_app(&apps, &exes, "Spotify.exe", |_| false),
        None
    );
}
