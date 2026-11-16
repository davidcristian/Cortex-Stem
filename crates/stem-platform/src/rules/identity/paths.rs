//! Paths and shell names read as strings: `shell:AppsFolder\<aumid>`, file
//! names and extensions, the folder an executable starts in, and `%NAME%`
//! references.

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

/// The folder an executable starts in when its launch target names none:
/// the exe's own folder, as Explorer and the stock taskbar use,
/// instead of whatever folder Stem itself runs in (System32 when Windows
/// starts it at sign-in). A drive root keeps its separator (`C:\app.exe` →
/// `C:\`; a bare `C:` would mean that drive's current folder). Quotes and
/// surrounding blanks are ignored; `None` for a bare file name or a path
/// that ends in a separator.
pub fn default_working_dir(exe_path: &str) -> Option<&str> {
    let path = exe_path.trim().trim_matches('"').trim();
    let at = path.rfind(['\\', '/'])?;
    let (dir, file) = (&path[..at], &path[at + 1..]);
    if dir.is_empty() || file.is_empty() {
        return None;
    }
    Some(if dir.ends_with(':') {
        &path[..=at]
    } else {
        dir
    })
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
