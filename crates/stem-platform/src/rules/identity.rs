//! Identity parsing helpers.
//!
//! `shell:AppsFolder\<aumid>` parsing, environment-variable detection for
//! shortcut targets, display-name fallbacks, version-resource lookups,
//! icon-key construction, Windows command-line splitting/joining, and the
//! packaged-executable rules (package full name from a path under
//! `WindowsApps`, which of a package's applications an executable belongs
//! to). Pure and tested here.
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

/// The name shown when nothing better is known.
pub const PLACEHOLDER_DISPLAY_NAME: &str = "Application";

/// A display name fit to show: trimmed, with every trailing `.exe`
/// (case-insensitive) removed, and non-empty. `None` when nothing is left.
/// Every name the identity code produces passes through here, so no item is
/// ever titled `Notepad.exe`: the file name of an unreadable
/// executable, a window title that is a program name, a shortcut called
/// `app.exe.lnk` and a version resource that repeats the file name are all
/// cleaned the same way.
pub fn clean_display_name(name: &str) -> Option<String> {
    let mut name = name.trim();
    while let Some(stripped) = strip_exe_suffix(name) {
        name = stripped.trim_end();
    }
    (!name.is_empty()).then(|| name.to_owned())
}

/// `name` without one trailing `.exe` (any case); `None` when it has none.
fn strip_exe_suffix(name: &str) -> Option<&str> {
    let cut = name.len().checked_sub(4)?;
    let (head, tail) = name.split_at_checked(cut)?;
    tail.eq_ignore_ascii_case(".exe").then_some(head)
}

/// First candidate that [`clean_display_name`] accepts.
pub fn first_display_name<'a, I>(candidates: I) -> Option<String>
where
    I: IntoIterator<Item = Option<&'a str>>,
{
    candidates
        .into_iter()
        .flatten()
        .find_map(clean_display_name)
}

/// Display name when no version resource is available: exe stem, then the
/// window title, then the class name, then a fixed placeholder; each
/// candidate cleaned by [`clean_display_name`].
pub fn display_name_fallback(exe_path: Option<&str>, title: &str, class_name: &str) -> String {
    let stem = exe_path.map(file_stem);
    first_display_name([stem.as_deref(), Some(title), Some(class_name)])
        .unwrap_or_else(|| PLACEHOLDER_DISPLAY_NAME.to_owned())
}

/// A readable fallback name for an AppUserModelID when the shell cannot
/// resolve it: `Microsoft.WindowsCalculator_8wekyb3d8bbwe!App` →
/// `WindowsCalculator`, `Chrome.Profile1` → `Profile1`, `Chrome` → `Chrome`.
pub fn aumid_fallback_name(aumid: &str) -> String {
    let Some(aumid) = clean_display_name(aumid) else {
        return PLACEHOLDER_DISPLAY_NAME.to_owned();
    };
    let family = aumid.split('!').next().unwrap_or(&aumid);
    let name = family.split('_').next().unwrap_or(family);
    let last = name.rsplit('.').next().unwrap_or(name);
    let chosen = if last.is_empty() { &aumid } else { last };
    first_display_name([Some(chosen), Some(&aumid)])
        .unwrap_or_else(|| PLACEHOLDER_DISPLAY_NAME.to_owned())
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

// ----- Packaged executables ---------------------------------------------------

/// The directory that holds installed packages (`%ProgramFiles%\WindowsApps`,
/// or `<drive>:\WindowsApps` for apps moved to another drive).
pub const WINDOWS_APPS_DIR: &str = "WindowsApps";

/// Where an executable lies inside an installed package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackagedExe<'a> {
    /// The package directory under `WindowsApps`, which is the package full
    /// name (`Microsoft.WindowsTerminal_1.24.12741.0_x64__8wekyb3d8bbwe`).
    pub package_full_name: &'a str,
    /// The rest of the path, relative to the package root, without leading
    /// or trailing separators (`WindowsTerminal.exe`, `app\Claude.exe`).
    pub relative_path: &'a str,
}

/// The package an executable belongs to, read from its path: the first
/// `WindowsApps` component (any case, either separator) followed by a
/// component shaped like a package full name ([`is_package_full_name`]) and
/// at least one more component. `None` for every other path, including the
/// app execution aliases under `%LOCALAPPDATA%\Microsoft\WindowsApps` (a file
/// name or a family-name directory follows there, not a full name).
pub fn packaged_exe(path: &str) -> Option<PackagedExe<'_>> {
    let path = path.trim();
    // (byte offset, component); separators are one ASCII byte each.
    let mut components = Vec::new();
    let mut offset = 0;
    for component in path.split(['/', '\\']) {
        components.push((offset, component));
        offset += component.len() + 1;
    }
    components.windows(3).find_map(|w| {
        let [(_, dir), (_, full_name), (rest_at, _)] = w else {
            return None;
        };
        if !dir.eq_ignore_ascii_case(WINDOWS_APPS_DIR) || !is_package_full_name(full_name) {
            return None;
        }
        let relative_path = path.get(*rest_at..)?.trim_matches(['/', '\\']);
        (!relative_path.is_empty()).then_some(PackagedExe {
            package_full_name: full_name,
            relative_path,
        })
    })
}

/// Whether `name` has the shape of a package full name: five `_`-separated
/// fields `<name>_<version>_<architecture>_<resource id>_<publisher id>`,
/// where the name is non-empty, the version is four dot-separated numbers,
/// the architecture and the publisher id are non-empty and alphanumeric, and
/// the resource id (usually empty) may be anything without a `_`. The OS
/// validates the rest when the name is used.
pub fn is_package_full_name(name: &str) -> bool {
    let fields: Vec<&str> = name.split('_').collect();
    let [package, version, architecture, _resource_id, publisher_id] = fields.as_slice() else {
        return false;
    };
    let alphanumeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric());
    let version_parts: Vec<&str> = version.split('.').collect();
    !package.trim().is_empty()
        && version_parts.len() == 4
        && version_parts.iter().all(|p| p.parse::<u16>().is_ok())
        && alphanumeric(architecture)
        && alphanumeric(publisher_id)
}

/// The full AppUserModelID for one entry of `GetPackageApplicationIds`. The
/// API returns full AUMIDs (`<family>!<app id>`; measured on Windows 11 build
/// 26300), but its documentation only says "app IDs", so an entry without a
/// `!` is taken as package-relative and prefixed with the family name. `None`
/// for a blank entry.
pub fn package_app_aumid(family: &str, entry: &str) -> Option<String> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }
    if entry.contains('!') {
        return Some(entry.to_owned());
    }
    Some(format!("{}!{entry}", family.trim()))
}

/// The strings an API wrote into `buffer` together with a table of pointers
/// to them (`GetPackagesByPackageFamily`, `GetPackageApplicationIds`).
/// `base` is the address of `buffer[0]`; every address must point inside
/// the buffer at a UTF-16 boundary and the string must end in a NUL inside
/// it, or the entry is skipped. Empty strings are skipped. Reads nothing
/// through the pointers themselves.
pub fn strings_at(buffer: &[u16], base: usize, addresses: &[usize]) -> Vec<String> {
    addresses
        .iter()
        .filter_map(|&address| {
            let offset = address.checked_sub(base)?;
            if offset % 2 != 0 {
                return None;
            }
            let units = buffer.get(offset / 2..)?;
            let end = units.iter().position(|&u| u == 0)?;
            (end > 0).then(|| String::from_utf16_lossy(&units[..end]))
        })
        .collect()
}

/// Whether two package-relative paths name the same file: either separator,
/// no leading or trailing separators, compared without regard to case.
pub fn same_relative_path(a: &str, b: &str) -> bool {
    let normalise = |s: &str| {
        s.trim()
            .trim_matches(['/', '\\'])
            .replace('/', "\\")
            .to_lowercase()
    };
    normalise(a) == normalise(b)
}

/// Pick the application of an installed package that a packaged executable
/// belongs to, or `None` when that is not certain. `apps` are the package's
/// full AUMIDs (`GetPackageApplicationIds`), `executables` pairs AUMIDs with
/// the manifest's `Executable` attribute (empty when the manifest could not
/// be read), `relative_exe` is the executable's path inside the package and
/// `shell_knows` says whether `shell:AppsFolder\<aumid>` resolves (asked only
/// when it decides):
/// 1. the package's only application;
/// 2. else the only application whose `Executable` is this executable;
/// 3. else, among the applications whose `Executable` is this executable
///    (all of them when none is), the only one the shell knows.
pub fn choose_package_app<'a>(
    apps: &'a [String],
    executables: &[(String, String)],
    relative_exe: &str,
    mut shell_knows: impl FnMut(&str) -> bool,
) -> Option<&'a str> {
    if let [only] = apps {
        return Some(only.as_str());
    }
    let runs_this_exe = |aumid: &str| {
        executables
            .iter()
            .any(|(a, exe)| a.eq_ignore_ascii_case(aumid) && same_relative_path(exe, relative_exe))
    };
    let by_exe: Vec<&'a str> = apps
        .iter()
        .map(String::as_str)
        .filter(|&a| runs_this_exe(a))
        .collect();
    if let [only] = by_exe.as_slice() {
        return Some(*only);
    }
    let candidates = if by_exe.is_empty() {
        apps.iter().map(String::as_str).collect()
    } else {
        by_exe
    };
    let known: Vec<&'a str> = candidates.into_iter().filter(|&a| shell_knows(a)).collect();
    match known.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

// ----- App execution alias redirects ------------------------------------------

/// The reparse tag of an app execution alias
/// (`%LOCALAPPDATA%\Microsoft\WindowsApps\<name>.exe`):
/// `IO_REPARSE_TAG_APPEXECLINK` in `Win32::System::SystemServices`.
pub const IO_REPARSE_TAG_APPEXECLINK: u32 = 0x8000_001B;

/// What an app execution alias points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppExecLink {
    /// `Microsoft.WindowsNotepad_8wekyb3d8bbwe`.
    pub package_family: String,
    /// `Microsoft.WindowsNotepad_8wekyb3d8bbwe!App`.
    pub app_user_model_id: String,
    /// The packaged executable it starts.
    pub target: String,
}

/// Parse the `REPARSE_DATA_BUFFER` that `FSCTL_GET_REPARSE_POINT` returns
/// for an app execution alias: the tag (`u32`), the data length (`u16`), a
/// reserved `u16`, then the data, little-endian: a version (`u32`; 3 on
/// Windows 11 build 26300, read with `fsutil reparsepoint query`) followed
/// by NUL-terminated UTF-16 strings, the package family name, the
/// AppUserModelID and the target executable (version 3 adds the app type).
/// `None` for another tag, a buffer shorter than its length says, fewer
/// than three strings, or a blank AppUserModelID.
pub fn parse_app_exec_link(buffer: &[u8]) -> Option<AppExecLink> {
    let tag = u32::from_le_bytes(buffer.get(0..4)?.try_into().ok()?);
    if tag != IO_REPARSE_TAG_APPEXECLINK {
        return None;
    }
    let length = usize::from(u16::from_le_bytes(buffer.get(4..6)?.try_into().ok()?));
    let data = buffer.get(8..8 + length)?;
    let units: Vec<u16> = data
        .get(4..)?
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let mut strings = units.split(|&unit| unit == 0).map(String::from_utf16_lossy);
    let package_family = strings.next()?;
    let app_user_model_id = strings.next()?.trim().to_owned();
    let target = strings.next()?;
    (!app_user_model_id.is_empty()).then_some(AppExecLink {
        package_family,
        app_user_model_id,
        target,
    })
}

/// Whether an Image File Execution Options filter (`FilterFullPath`) names
/// `exe_path`: the same path in any case, with either separator, quotes and
/// surrounding blanks ignored.
pub fn alias_filter_matches(filter_full_path: &str, exe_path: &str) -> bool {
    let normalise = |s: &str| {
        s.trim()
            .trim_matches('"')
            .trim()
            .replace('/', "\\")
            .to_lowercase()
    };
    let filter = normalise(filter_full_path);
    !filter.is_empty() && filter == normalise(exe_path)
}

/// Whether `path` is an executable somewhere under `<drive>:\Windows\`, where
/// Windows keeps the inbox executables an app execution alias may redirect
/// to a packaged app (Windows 11's `C:\Windows\notepad.exe` starts the Store
/// Notepad). A cheap test for which pins are worth asking about again; the
/// platform decides.
pub fn windows_dir_exe(path: &str) -> bool {
    let mut components = path.trim().trim_matches('"').split(['/', '\\']);
    let drive = components.next().unwrap_or_default();
    let is_drive =
        drive.len() == 2 && drive.ends_with(':') && drive.as_bytes()[0].is_ascii_alphabetic();
    is_drive
        && components
            .next()
            .is_some_and(|dir| dir.eq_ignore_ascii_case("Windows"))
        && components.next().is_some_and(|rest| !rest.is_empty())
        && has_extension(path.trim().trim_matches('"'), "exe")
}

/// Where each part of a shortcut's identity comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutSource {
    /// The `System.AppUserModel.ID` property of the `.lnk`.
    AppUserModelId,
    /// The resolved target executable (path, arguments, working directory).
    Target,
    /// The `.lnk` file itself.
    Link,
}

/// How to identify a `.lnk`, per part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutPlan {
    /// `AppUserModelId` → `aumid:<id>`; `Target` → `exe:<target>`; `Link` →
    /// `exe:<lnk path>`.
    pub id: ShortcutSource,
    /// `AppUserModelId` → the AppsFolder display name; `Target` → the
    /// target's version resource, then the link name; `Link` → the link name.
    pub name: ShortcutSource,
    /// `AppUserModelId` → `aumid:`; `Target` → `exe:`; `Link` → `file:`.
    pub icon: ShortcutSource,
    /// `Target` → `Exe`; `AppUserModelId` → `StoreApp`; `Link` → `Shortcut`.
    pub launch: ShortcutSource,
}

/// Decide how a shortcut is identified from three facts: whether it has a
/// target path, its `System.AppUserModel.ID` property (if set) and whether
/// the shell's AppsFolder knows that AUMID.
///
/// The **id** follows the AUMID whenever one is set, even with a target:
/// apps that stamp an AUMID on their shortcut (VS Code, Chrome profiles,
/// most Electron apps) stamp the same one on their windows, and the engine
/// groups those windows by AUMID (`stem_core::app_id_for`). The **launch**
/// prefers the resolved exe (more reliable than activating a non-packaged
/// AUMID). **Name** and **icon** come from the AppsFolder when the shell
/// knows the AUMID, else from the target, else from the link.
pub fn plan_shortcut(
    has_target: bool,
    aumid: Option<&str>,
    apps_folder_known: bool,
) -> ShortcutPlan {
    let has_aumid = aumid.is_some_and(|a| !a.trim().is_empty());
    let shell_knows = has_aumid && apps_folder_known;
    let id = if has_aumid {
        ShortcutSource::AppUserModelId
    } else if has_target {
        ShortcutSource::Target
    } else {
        ShortcutSource::Link
    };
    let looks = if shell_knows {
        ShortcutSource::AppUserModelId
    } else if has_target {
        ShortcutSource::Target
    } else {
        ShortcutSource::Link
    };
    let launch = if has_target {
        ShortcutSource::Target
    } else if has_aumid {
        ShortcutSource::AppUserModelId
    } else {
        ShortcutSource::Link
    };
    ShortcutPlan {
        id,
        name: looks,
        icon: looks,
        launch,
    }
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
            packaged_exe(
                r"D:\WindowsApps\Vendor.App_1.0.0.0_neutral_~_abcdefghijklm\bin\\app.exe\"
            ),
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

    #[test]
    fn shortcut_plan_groups_by_aumid_and_launches_the_target() {
        use ShortcutSource::*;

        // VS Code / Chrome profile: target + AUMID the shell knows.
        assert_eq!(
            plan_shortcut(true, Some("Chrome.Profile1"), true),
            ShortcutPlan {
                id: AppUserModelId,
                name: AppUserModelId,
                icon: AppUserModelId,
                launch: Target,
            }
        );
        // Target + AUMID the shell does not know: grouped by AUMID, but the
        // exe supplies name, icon and launch.
        assert_eq!(
            plan_shortcut(true, Some("Vendor.App"), false),
            ShortcutPlan {
                id: AppUserModelId,
                name: Target,
                icon: Target,
                launch: Target,
            }
        );
        // Plain shortcut to an exe.
        assert_eq!(
            plan_shortcut(true, None, false),
            ShortcutPlan {
                id: Target,
                name: Target,
                icon: Target,
                launch: Target,
            }
        );
        // `apps_folder_known` is meaningless without an AUMID.
        assert_eq!(
            plan_shortcut(true, None, true),
            plan_shortcut(true, None, false)
        );
        assert_eq!(
            plan_shortcut(true, Some("  "), true),
            plan_shortcut(true, None, false)
        );
        // Store-app shortcut: no target, AUMID known to the shell.
        assert_eq!(
            plan_shortcut(
                false,
                Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
                true
            ),
            ShortcutPlan {
                id: AppUserModelId,
                name: AppUserModelId,
                icon: AppUserModelId,
                launch: AppUserModelId,
            }
        );
        // No target, AUMID unknown to the shell: still launched by AUMID,
        // named and drawn from the link.
        assert_eq!(
            plan_shortcut(false, Some("Gone.App"), false),
            ShortcutPlan {
                id: AppUserModelId,
                name: Link,
                icon: Link,
                launch: AppUserModelId,
            }
        );
        // Nothing to go on: the link itself.
        assert_eq!(
            plan_shortcut(false, None, false),
            ShortcutPlan {
                id: Link,
                name: Link,
                icon: Link,
                launch: Link,
            }
        );
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
