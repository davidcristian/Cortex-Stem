//! App execution aliases: the reparse point that names a packaged app, and
//! the executables under the Windows folder that an alias may redirect.

use super::paths::has_extension;

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
