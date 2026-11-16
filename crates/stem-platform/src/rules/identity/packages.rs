//! Packaged executables: the package full name read from a path under
//! `WindowsApps`, and which of a package's applications an executable
//! belongs to.

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
