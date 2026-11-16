//! App execution aliases: the package application Windows starts instead
//! of an inbox executable, read from the registry and the alias file.

use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;

use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, MAXIMUM_REPARSE_DATA_BUFFER_SIZE,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey,
    RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};
use windows::core::{PCWSTR, PWSTR};

use super::names::apps_folder_display_name;
use crate::rules::identity::{
    AppExecLink, alias_filter_matches, file_name, has_extension, parse_app_exec_link,
};
use crate::windows::util;

/// The Image File Execution Options key, under `HKEY_LOCAL_MACHINE`.
const IFEO_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";
/// At most this many filter subkeys are read under one IFEO entry (Windows
/// 11 build 26300 has three for `notepad.exe`: System32, SysWOW64 and the
/// Windows folder).
const MAX_IFEO_FILTERS: u32 = 32;
/// Room for a registry key name (at most 255 characters) and its NUL.
const REG_NAME_BUFFER: usize = 256;
/// Room for a path read from the registry, in UTF-16 units.
const REG_PATH_BUFFER: usize = 1024;

/// The package application Windows starts instead of the inbox executable
/// at `path` (Windows 11's `C:\Windows\notepad.exe` starts the Store
/// Notepad, whose windows carry its `aumid:` id): the Image File Execution
/// Options entry for the file name has `UseFilter = 1` and a filter subkey
/// whose `FilterFullPath` is this path with `AppExecutionAliasRedirect = 1`,
/// and the user's app execution alias of that name
/// (`%LOCALAPPDATA%\Microsoft\WindowsApps\<name>`, an
/// `IO_REPARSE_TAG_APPEXECLINK` reparse point) names an AppUserModelID the
/// AppsFolder knows. `None` otherwise: no redirect, the alias turned off in
/// Windows Settings (then the inbox exe really runs, and its `exe:`
/// identity is right), or a package that is gone. Bounded: two registry
/// reads per filter subkey, one small file read and one AppsFolder lookup.
pub(super) fn alias_redirect_aumid(path: &str) -> Option<String> {
    let name = file_name(path);
    if !has_extension(name, "exe") || !ifeo_redirects(name, path) {
        return None;
    }
    let local = std::env::var("LOCALAPPDATA").ok()?;
    let alias = format!(
        r"{}\Microsoft\WindowsApps\{name}",
        local.trim_end_matches(['\\', '/'])
    );
    let Some(link) = read_app_exec_link(&alias) else {
        log::debug!(
            "{path}: Windows redirects it to an app execution alias, but {alias} names no package app (the alias is turned off?)"
        );
        return None;
    };
    let aumid = link.app_user_model_id;
    if apps_folder_display_name(&aumid).is_none() {
        log::debug!("{path}: its alias names {aumid:?}, which the AppsFolder does not know");
        return None;
    }
    Some(aumid)
}

/// Whether the Image File Execution Options entry for the file `name`
/// redirects `path` to an app execution alias ([`alias_redirect_aumid`]).
fn ifeo_redirects(name: &str, path: &str) -> bool {
    let Some(key) = RegKey::open(HKEY_LOCAL_MACHINE, &format!(r"{IFEO_KEY}\{name}")) else {
        return false;
    };
    if key.dword(None, "UseFilter") != Some(1) {
        return false;
    }
    (0..MAX_IFEO_FILTERS)
        .map_while(|index| key.subkey_name(index))
        .any(|filter| {
            key.dword(Some(&filter), "AppExecutionAliasRedirect") == Some(1)
                && key
                    .string(Some(&filter), "FilterFullPath")
                    .is_some_and(|full| alias_filter_matches(&full, path))
        })
}

/// An open registry key, closed on drop.
struct RegKey(HKEY);

impl RegKey {
    /// `path` under `parent`, opened for reading; `None` when it is missing
    /// or cannot be read.
    fn open(parent: HKEY, path: &str) -> Option<Self> {
        let wide = util::to_wide(path);
        let mut key = HKEY::default();
        // SAFETY: `wide` is NUL-terminated and outlives the call; `key` is a
        // live out pointer the call fills on success only.
        let status =
            unsafe { RegOpenKeyExW(parent, util::pcwstr(&wide), None, KEY_READ, &mut key) };
        (status == ERROR_SUCCESS).then_some(Self(key))
    }

    /// The name of subkey `index`; `None` past the last one (or for a name
    /// that does not fit, which no IFEO filter has).
    fn subkey_name(&self, index: u32) -> Option<String> {
        let mut buffer = [0u16; REG_NAME_BUFFER];
        let mut len = REG_NAME_BUFFER as u32;
        // SAFETY: `buffer` holds `len` characters, which the call is told;
        // the class and the time are not asked for.
        let status = unsafe {
            RegEnumKeyExW(
                self.0,
                index,
                Some(PWSTR(buffer.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        let len = (len as usize).min(buffer.len());
        (status == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&buffer[..len]))
    }

    /// The `REG_DWORD` `value` of this key, or of its `subkey`.
    fn dword(&self, subkey: Option<&str>, value: &str) -> Option<u32> {
        let subkey = subkey.map(util::to_wide);
        let value = util::to_wide(value);
        let mut data = 0u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: both names are NUL-terminated and outlive the call (a null
        // subkey reads this key); `data` is writable for `size` bytes, and
        // RRF_RT_REG_DWORD makes a value of another type fail instead.
        let status = unsafe {
            RegGetValueW(
                self.0,
                subkey.as_deref().map_or(PCWSTR::null(), util::pcwstr),
                util::pcwstr(&value),
                RRF_RT_REG_DWORD,
                None,
                Some(std::ptr::from_mut(&mut data).cast()),
                Some(&mut size),
            )
        };
        (status == ERROR_SUCCESS).then_some(data)
    }

    /// The `REG_SZ` `value` of this key, or of its `subkey`.
    fn string(&self, subkey: Option<&str>, value: &str) -> Option<String> {
        let subkey = subkey.map(util::to_wide);
        let value = util::to_wide(value);
        let mut buffer = vec![0u16; REG_PATH_BUFFER];
        let mut size = u32::try_from(buffer.len() * size_of::<u16>()).ok()?;
        // SAFETY: both names are NUL-terminated and outlive the call;
        // `buffer` is writable for `size` bytes, and RRF_RT_REG_SZ makes a
        // value of another type fail instead (the call NUL-terminates it).
        let status = unsafe {
            RegGetValueW(
                self.0,
                subkey.as_deref().map_or(PCWSTR::null(), util::pcwstr),
                util::pcwstr(&value),
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        (status == ERROR_SUCCESS).then(|| util::from_wide(&buffer))
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: the key was opened by `RegKey::open` and is closed once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// What the app execution alias at `path` points at
/// (`rules::identity::parse_app_exec_link`); `None` when the file is
/// missing, is not such an alias, or cannot be read.
fn read_app_exec_link(path: &str) -> Option<AppExecLink> {
    // The reparse point itself (a directory too), not what it points at;
    // the `File` closes the handle.
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0)
        .open(path)
        .inspect_err(|e| log::trace!("{path}: not opened as a reparse point: {e}"))
        .ok()?;
    let mut buffer = vec![0u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize];
    let capacity = u32::try_from(buffer.len()).ok()?;
    let mut returned = 0u32;
    // SAFETY: synchronous call on the live handle of `file`, opened without
    // FILE_FLAG_OVERLAPPED; `buffer` is writable for `capacity` bytes, which
    // the call is told, and `returned` is a live out pointer.
    unsafe {
        DeviceIoControl(
            HANDLE(file.as_raw_handle()),
            FSCTL_GET_REPARSE_POINT,
            None,
            0,
            Some(buffer.as_mut_ptr().cast()),
            capacity,
            Some(&mut returned),
            None,
        )
    }
    .inspect_err(|e| log::trace!("{path}: FSCTL_GET_REPARSE_POINT failed: {e}"))
    .ok()?;
    buffer.truncate((returned as usize).min(buffer.len()));
    parse_app_exec_link(&buffer)
}
