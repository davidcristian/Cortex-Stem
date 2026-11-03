//! Identity parsing helpers.
//!
//! `shell:AppsFolder\<aumid>` parsing, environment-variable detection for
//! shortcut targets, display-name fallbacks, version-resource lookups,
//! icon-key construction and Windows command-line splitting/joining. Pure
//! and tested here.
//!
//! Paths are handled as strings on purpose: `std::path::Path` only treats
//! `\` as a separator on Windows, and these helpers must behave the same in
//! tests on any host (`docs/development/failure-modes.md`).

use stem_core::IconKey;

/// Prefix of the parsing name of an entry in the shell's Applications folder.
pub const APPS_FOLDER_PREFIX: &str = r"shell:AppsFolder\";

/// The AppUserModelID inside `shell:AppsFolder\<aumid>` (prefix matched
/// case-insensitively, either slash accepted), or `None` for anything else.
pub fn parse_apps_folder(uri: &str) -> Option<&str> {
    let uri = uri.trim();
    let (prefix, rest) = uri.split_at_checked(APPS_FOLDER_PREFIX.len() - 1)?;
    if !prefix.eq_ignore_ascii_case(&APPS_FOLDER_PREFIX[..APPS_FOLDER_PREFIX.len() - 1]) {
        return None;
    }
    let aumid = rest.strip_prefix(['\\', '/'])?.trim();
    (!aumid.is_empty()).then_some(aumid)
}

/// Parsing name for `SHCreateItemFromParsingName` / `ShellExecuteExW`.
pub fn apps_folder_parsing_name(aumid: &str) -> String {
    format!("{APPS_FOLDER_PREFIX}{}", aumid.trim())
}

/// Last path component, treating `/` and `\` as separators and ignoring a
/// trailing separator.
pub fn file_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed)
}

/// File name without its extension (`C:\A\b.exe` → `b`). Dot-files keep
/// their name.
pub fn file_stem(path: &str) -> String {
    let name = file_name(path);
    match name.rfind('.') {
        Some(0) | None => name.to_owned(),
        Some(dot) => name[..dot].to_owned(),
    }
}

/// Case-insensitive extension test; `ext` is given without the dot.
pub fn has_extension(path: &str, ext: &str) -> bool {
    let name = file_name(path);
    match name.rfind('.') {
        Some(0) | None => false,
        Some(dot) => name[dot + 1..].eq_ignore_ascii_case(ext),
    }
}

/// `true` when the string contains a `%NAME%` reference worth expanding.
pub fn needs_env_expansion(s: &str) -> bool {
    let mut parts = s.split('%');
    // Skip the text before the first `%`.
    parts.next();
    let mut expect_name = true;
    for part in parts {
        if expect_name
            && !part.is_empty()
            && !part.contains(|c: char| c.is_whitespace() || c == '/' || c == '\\')
        {
            return true;
        }
        expect_name = !expect_name;
    }
    false
}

/// First candidate that is non-empty after trimming.
pub fn first_non_empty<'a, I>(candidates: I) -> Option<String>
where
    I: IntoIterator<Item = Option<&'a str>>,
{
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Display name when no version resource is available: exe stem, then the
/// window title, then the class name, then a fixed placeholder.
pub fn display_name_fallback(exe_path: Option<&str>, title: &str, class_name: &str) -> String {
    let stem = exe_path.map(file_stem);
    first_non_empty([stem.as_deref(), Some(title), Some(class_name)])
        .unwrap_or_else(|| "Application".to_owned())
}

/// A readable fallback name for an AppUserModelID when the shell cannot
/// resolve it: `Microsoft.WindowsCalculator_8wekyb3d8bbwe!App` →
/// `WindowsCalculator`, `Chrome.Profile1` → `Profile1`, `Chrome` → `Chrome`.
pub fn aumid_fallback_name(aumid: &str) -> String {
    let aumid = aumid.trim();
    let family = aumid.split('!').next().unwrap_or(aumid);
    let name = family.split('_').next().unwrap_or(family);
    let last = name.rsplit('.').next().unwrap_or(name);
    let chosen = if last.is_empty() { aumid } else { last };
    chosen.to_owned()
}

/// Sub-block path for `VerQueryValueW`: `\StringFileInfo\{lang}{codepage}\{name}`.
pub fn version_string_path(lang: u16, codepage: u16, name: &str) -> String {
    format!(r"\StringFileInfo\{lang:04x}{codepage:04x}\{name}")
}

/// Parse the `\VarFileInfo\Translation` block: an array of
/// `(language, code page)` 16-bit pairs, little-endian. Trailing bytes that
/// do not form a pair are ignored.
pub fn translations(block: &[u8]) -> Vec<(u16, u16)> {
    block
        .chunks_exact(4)
        .map(|c| {
            (
                u16::from_le_bytes([c[0], c[1]]),
                u16::from_le_bytes([c[2], c[3]]),
            )
        })
        .collect()
}

/// Translations to try, in order: the file's own first, then the usual
/// suspects (US English in Unicode and in the Windows-1252 code page, and the
/// neutral language), without duplicates.
pub fn translation_candidates(declared: &[(u16, u16)]) -> Vec<(u16, u16)> {
    let mut out: Vec<(u16, u16)> = Vec::with_capacity(declared.len() + 3);
    for t in declared
        .iter()
        .copied()
        .chain([(0x0409, 0x04B0), (0x0409, 0x04E4), (0x0000, 0x04B0)])
    {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Version-resource string names consulted for a display name, best first.
pub const VERSION_NAME_FIELDS: [&str; 2] = ["FileDescription", "ProductName"];

pub fn exe_icon_key(path: &str) -> IconKey {
    IconKey(format!("exe:{path}"))
}

pub fn file_icon_key(path: &str) -> IconKey {
    IconKey(format!("file:{path}"))
}

pub fn aumid_icon_key(aumid: &str) -> IconKey {
    IconKey(format!("aumid:{}", aumid.trim()))
}

/// Key of a window's own icon: `hwnd:<id>:<pid>`. The process id is part of
/// the key because HWND values are recycled by the OS; a key that outlives
/// its window must not resolve to a different window's icon.
pub fn hwnd_icon_key(id: u64, process_id: u32) -> IconKey {
    IconKey(format!("hwnd:{id}:{process_id}"))
}

/// The window id inside the value of an `hwnd:` key, accepting both the
/// current `<id>:<pid>` form and the older bare `<id>` (the pid is ignored).
pub fn parse_hwnd_icon_value(value: &str) -> Option<u64> {
    let id = value.split_once(':').map_or(value, |(id, _)| id);
    id.trim().parse().ok()
}

/// Split a Windows command line into arguments following the
/// `CommandLineToArgvW` rules: whitespace separates, double quotes group,
/// `2n` backslashes before a quote become `n` backslashes, `2n+1` become `n`
/// backslashes plus a literal quote, and `""` inside a quoted run is a
/// literal quote.
pub fn split_command_line(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_arg = false;
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let mut backslashes = 1;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    backslashes += 1;
                }
                if chars.peek() == Some(&'"') {
                    current.extend(std::iter::repeat_n('\\', backslashes / 2));
                    if backslashes % 2 == 1 {
                        chars.next();
                        current.push('"');
                    }
                } else {
                    current.extend(std::iter::repeat_n('\\', backslashes));
                }
                in_arg = true;
            }
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = !in_quotes;
                }
                in_arg = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            c => {
                current.push(c);
                in_arg = true;
            }
        }
    }
    if in_arg {
        args.push(current);
    }
    args
}

/// Quote one argument so that [`split_command_line`] (and the C runtime)
/// gets it back unchanged.
pub fn quote_argument(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains(|c: char| c.is_whitespace() || c == '"') {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            c => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

/// Join arguments into a command line that [`split_command_line`] inverts.
pub fn join_command_line(args: &[String]) -> String {
    args.iter()
        .map(|a| quote_argument(a))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
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
    fn command_line_splitting_follows_msvcrt_rules() {
        assert_eq!(split_command_line(""), Vec::<String>::new());
        assert_eq!(split_command_line("   "), Vec::<String>::new());
        assert_eq!(split_command_line("a b  c"), vec!["a", "b", "c"]);
        assert_eq!(
            split_command_line(r#""C:\Program Files\x.exe" --flag "two words""#),
            vec![r"C:\Program Files\x.exe", "--flag", "two words"]
        );
        assert_eq!(split_command_line(r#"a\\b"#), vec![r"a\\b"]);
        assert_eq!(split_command_line(r#"a\\"b c"#), vec![r"a\b c"]);
        assert_eq!(split_command_line(r#"a\"b c"#), vec![r#"a"b"#, "c"]);
        assert_eq!(split_command_line(r#""a""b" c"#), vec![r#"a"b"#, "c"]);
        assert_eq!(split_command_line(r#""" x"#), vec!["", "x"]);
        assert_eq!(
            split_command_line(r#"--profile-directory="Profile 1""#),
            vec!["--profile-directory=Profile 1"]
        );
    }

    #[test]
    fn command_line_join_round_trips() {
        let cases: Vec<Vec<String>> = vec![
            vec![],
            vec!["plain".into()],
            vec!["".into(), "x".into()],
            vec!["two words".into(), r"C:\path\".into()],
            vec![r#"say "hi""#.into(), r#"back\slash\"quote"#.into()],
            vec![r"trailing\\".into(), "tab\there".into()],
        ];
        for args in cases {
            let line = join_command_line(&args);
            assert_eq!(split_command_line(&line), args, "line: {line}");
        }
        assert_eq!(quote_argument("plain"), "plain");
        assert_eq!(quote_argument(""), r#""""#);
        assert_eq!(quote_argument("two words"), r#""two words""#);
        // A trailing backslash is literal unless a quote follows it.
        assert_eq!(quote_argument(r"C:\dir\"), r"C:\dir\");
        assert_eq!(quote_argument(r"C:\my dir\"), r#""C:\my dir\\""#);
        assert_eq!(join_command_line(&["a".into(), "b c".into()]), r#"a "b c""#);
    }
}
