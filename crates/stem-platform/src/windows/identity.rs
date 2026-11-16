//! Application identity.
//!
//! Contract:
//! - [`identify_window`] takes the `RawWindow` facts already collected by
//!   `enumerate` and produces an [`AppIdentity`]: `id` via
//!   `stem_core::app_id_for`, `display_name` from the AppsFolder for an
//!   AUMID the shell knows, else the exe's `FileDescription` version
//!   resource (fallback: `ProductName`, then file stem, then window title,
//!   then class name, never ending in `.exe`), `icon_key` (`aumid:` for
//!   packaged apps, otherwise `exe:`), and a `launch` target (`StoreApp` for
//!   packaged apps, otherwise `Exe`).
//! - [`identify_target`] resolves what the user wants to pin: `.lnk` files are
//!   resolved through `IShellLinkW` (target path, arguments, working dir, icon
//!   location); `shell:AppsFolder\<aumid>` and `StoreApp` map to `aumid:`;
//!   plain executables map to `exe:`. The returned `id` **must** equal the id
//!   the engine would derive for that app's windows, or pins and running
//!   windows will not merge.
//! - An `Exe` target under `\WindowsApps\<package full name>\` is
//!   the executable of a packaged app: `identify_target` returns that app's
//!   identity (`aumid:` id, AppsFolder name, `aumid:` icon, `StoreApp`
//!   launch) when it can tell which of the package's applications the
//!   executable belongs to (`rules::identity::choose_package_app`), and the
//!   plain exe identity otherwise. The package is found through its family,
//!   so a path naming a version that has since been updated still resolves.
//! - An argument-free `Exe` target that Windows redirects to a package app
//!   through an app execution alias (Windows 11's `C:\Windows\notepad.exe`
//!   and `System32\notepad.exe` start the Store Notepad) gets that app's
//!   identity too, so a dropped `notepad.exe` merges with Notepad's `aumid:`
//!   windows: the Image File Execution Options filter that redirects the
//!   path, then the user's alias of the same name, which names the AUMID
//!   ([`alias_redirect_aumid`]). With the alias turned off the exe really
//!   runs and keeps its `exe:` identity.
//! - Elevated/protected processes: `exe_path` may be `None`; degrade to the
//!   window title and class without failing.
//! - No display name ends in `.exe`: every name, from whichever
//!   source, goes through `rules::identity::clean_display_name`, and
//!   `PLACEHOLDER_DISPLAY_NAME` is the last resort.
//!
//! Decisions:
//! - Packaged Win32 apps (the Windows 11 Notepad, Terminal, Paint, Snipping
//!   Tool, Photos) carry no `System.AppUserModel.ID` window property;
//!   `enumerate` fills `RawWindow::app_user_model_id` from the process's
//!   package identity instead, so they take the AUMID branch below like UWP
//!   apps do: AppsFolder name, `aumid:` icon, `StoreApp` launch. Their exe
//!   under `WindowsApps` is unreadable for a normal user (no version
//!   resource, no icon), which is why the exe branch is wrong for them.
//! - A window with an AppUserModelID whose `shell:AppsFolder\<aumid>` item
//!   resolves (Store apps, and desktop apps that registered an AUMID such as
//!   browser profiles) gets its name from the shell, an `aumid:` icon and a
//!   `StoreApp` launch target (`launch` falls back to `ShellExecuteExW` for
//!   non-packaged AUMIDs). An AUMID the shell does not know falls back to the
//!   exe for name/icon/launch while keeping the AUMID-based id.
//! - Packaged executables: settings written by older builds pinned
//!   packaged apps by their exe under `WindowsApps`, which neither merges
//!   with the app's `aumid:` windows nor yields a name. The package full name
//!   is the directory under `WindowsApps`; `PackageFamilyNameFromFullName`,
//!   then `GetPackagesByPackageFamily` for the version installed now,
//!   `GetPackageApplicationIds` for its applications (full AUMIDs, measured
//!   on Windows 11 build 26300). With several applications the manifest's
//!   `Executable` attributes (`IAppxManifestReader`) pick the one that runs
//!   this exe, and the AppsFolder breaks a remaining tie; anything still
//!   ambiguous stays an `exe:` identity rather than a guess.
//! - Shortcuts keep the resolved exe + arguments as the launch target, not the
//!   `.lnk`, so the pinned id equals the running app's `exe:` id. A `.lnk`
//!   whose `System.AppUserModel.ID` property is set (VS Code, Chrome
//!   profiles, most Electron apps, Store apps) is identified by that AUMID
//!   even when it also has a target, because the engine groups such apps'
//!   windows by AUMID; the exe still supplies the launch target, and name
//!   and icon come from the AppsFolder when the shell knows the AUMID, else
//!   from the exe (`rules::identity::plan_shortcut`). `.url` files are not
//!   shell links; they stay `Shortcut` targets with a `file:` icon.
//! - An advertised (Windows Installer) shortcut (`SLDF_HAS_DARWINID`) has no
//!   usable path: `GetPath` names the icon file the installer cached under
//!   `C:\Windows\Installer\{ProductCode}\`. Its target is the installed key
//!   file Windows Installer names for it (`MsiGetShortcutTargetW`,
//!   `MsiGetComponentPathW`, installed locally and present), else none, so
//!   the `.lnk` itself is pinned and launched and the shell and Windows
//!   Installer start the program.
//! - Pure helpers (parsing, fallbacks, the shortcut decision, command-line
//!   splitting) live in `rules::identity` and are unit-tested on every host.

use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use stem_core::{AppId, AppIdentity, LaunchTarget, RawWindow};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE, HWND};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileVersionInfoSizeW,
    GetFileVersionInfoW, MAXIMUM_REPARSE_DATA_BUFFER_SIZE, VerQueryValueW,
};
use windows::Win32::Storage::Packaging::Appx::{
    _PACKAGE_INFO_REFERENCE, AppxFactory, ClosePackageInfo, GetPackageApplicationIds,
    GetPackagePathByFullName, GetPackagesByPackageFamily, IAppxFactory, OpenPackageInfoByFullName,
    PACKAGE_FAMILY_NAME_MAX_LENGTH, PackageFamilyNameFromFullName,
};
use windows::Win32::System::ApplicationInstallationAndServicing::{
    INSTALLSTATE_LOCAL, MAX_FEATURE_CHARS, MAX_GUID_CHARS, MsiGetComponentPathW,
    MsiGetShortcutTargetW,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ, STGM_SHARE_DENY_NONE,
};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey,
    RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    IShellItem, IShellLinkDataList, IShellLinkW, SHCreateItemFromParsingName,
    SHCreateStreamOnFileEx, SIGDN_NORMALDISPLAY, SLDF_HAS_DARWINID, SLGP_RAWPATH, SLR_NO_UI,
    SLR_NOUPDATE, ShellLink,
};
use windows::core::{Interface, PCWSTR, PWSTR, w};

use super::util::{self, CoTaskWideString};
use crate::rules::identity::{
    AppExecLink, PLACEHOLDER_DISPLAY_NAME, PackagedExe, ShortcutSource, VERSION_NAME_FIELDS,
    alias_filter_matches, apps_folder_parsing_name, aumid_fallback_name, aumid_icon_key,
    choose_package_app, clean_display_name, display_name_fallback, exe_icon_key, file_icon_key,
    file_name, file_stem, first_display_name, has_extension, hwnd_icon_key, needs_env_expansion,
    package_app_aumid, packaged_exe, parse_app_exec_link, parse_apps_folder, plan_shortcut,
    split_command_line, strings_at, translation_candidates, translations, version_string_path,
    version_string_value,
};
use crate::{PlatformError, PlatformResult};

/// Buffer size for `IShellLinkW` string getters (long paths included).
const LINK_BUFFER: usize = 2048;
/// `IShellLinkW::Resolve` budget in milliseconds (high word of the flags).
const RESOLVE_TIMEOUT_MS: u32 = 500;

pub fn identify_window(raw: &RawWindow) -> PlatformResult<AppIdentity> {
    let _com = util::ComApartment::sta();
    let id = stem_core::app_id_for(raw);
    let exe = raw
        .exe_path
        .as_deref()
        .map(|p| p.to_string_lossy().into_owned());

    if let Some(aumid) = raw
        .app_user_model_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(display_name) = apps_folder_display_name(aumid) {
            return Ok(AppIdentity {
                id,
                display_name,
                icon_key: Some(aumid_icon_key(aumid)),
                launch: Some(LaunchTarget::StoreApp {
                    app_user_model_id: aumid.to_owned(),
                }),
            });
        }
        log::debug!("AUMID {aumid:?} is not in the AppsFolder; using the exe for name/icon/launch");
    }

    let display_name = exe
        .as_deref()
        .and_then(exe_display_name)
        .unwrap_or_else(|| display_name_fallback(exe.as_deref(), &raw.title, &raw.class_name));
    let icon_key = exe
        .as_deref()
        .map(exe_icon_key)
        .or_else(|| Some(hwnd_icon_key(raw.id.0, raw.process_id)));
    let launch = exe.map(|path| LaunchTarget::Exe {
        path: PathBuf::from(path),
        args: Vec::new(),
        working_dir: None,
    });
    Ok(AppIdentity {
        id,
        display_name,
        icon_key,
        launch,
    })
}

pub fn identify_target(target: &LaunchTarget) -> PlatformResult<AppIdentity> {
    let _com = util::ComApartment::sta();
    match target {
        LaunchTarget::Exe {
            path,
            args,
            working_dir,
        } => {
            let path_s = path.to_string_lossy();
            if has_extension(&path_s, "lnk") {
                return identify_shortcut(path);
            }
            if let Some(packaged) = packaged_exe(&path_s)
                && let Some(aumid) = packaged_app_aumid(&packaged)
            {
                log::debug!("{path_s}: an executable of package application {aumid:?}");
                return Ok(identity_for_aumid(&aumid));
            }
            // A redirected exe starts its package app with no arguments of
            // ours to pass on, so only an argument-free target is mapped.
            if args.is_empty()
                && let Some(aumid) = alias_redirect_aumid(&path_s)
            {
                log::debug!(
                    "{path_s}: Windows redirects it to package application {aumid:?} (app execution alias)"
                );
                return Ok(identity_for_aumid(&aumid));
            }
            Ok(identity_for_exe(
                &path_s,
                args.clone(),
                working_dir.clone(),
                None,
            ))
        }
        LaunchTarget::Shortcut { path } => identify_shortcut(path),
        LaunchTarget::StoreApp { app_user_model_id } => Ok(identity_for_aumid(app_user_model_id)),
        LaunchTarget::Uri { uri } => {
            if let Some(aumid) = parse_apps_folder(uri) {
                return Ok(identity_for_aumid(aumid));
            }
            Ok(AppIdentity {
                id: AppId(format!("uri:{uri}")),
                display_name: uri.clone(),
                icon_key: None,
                launch: Some(target.clone()),
            })
        }
    }
}

/// Identity of a classic executable. `preferred_name` (e.g. the shortcut's
/// file name) ranks after the version resource and before the exe stem.
fn identity_for_exe(
    path: &str,
    args: Vec<String>,
    working_dir: Option<PathBuf>,
    preferred_name: Option<&str>,
) -> AppIdentity {
    let from_version = exe_display_name(path);
    let stem = file_stem(path);
    let display_name = first_display_name([from_version.as_deref(), preferred_name, Some(&stem)])
        .unwrap_or_else(|| PLACEHOLDER_DISPLAY_NAME.to_owned());
    AppIdentity {
        id: AppId::for_exe(Path::new(path)),
        display_name,
        icon_key: Some(exe_icon_key(path)),
        launch: Some(LaunchTarget::Exe {
            path: PathBuf::from(path),
            args,
            working_dir,
        }),
    }
}

/// Identity of a packaged app (or any AUMID the shell knows).
fn identity_for_aumid(aumid: &str) -> AppIdentity {
    let aumid = aumid.trim();
    let display_name =
        apps_folder_display_name(aumid).unwrap_or_else(|| aumid_fallback_name(aumid));
    AppIdentity {
        id: AppId::for_app_user_model_id(aumid),
        display_name,
        icon_key: Some(aumid_icon_key(aumid)),
        launch: Some(LaunchTarget::StoreApp {
            app_user_model_id: aumid.to_owned(),
        }),
    }
}

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
fn alias_redirect_aumid(path: &str) -> Option<String> {
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

/// Resolve a `.lnk` and identify what it points at. The decision (which of
/// the AUMID property, the target and the link itself supplies the id, the
/// name, the icon and the launch target) is `rules::identity::plan_shortcut`.
fn identify_shortcut(path: &Path) -> PlatformResult<AppIdentity> {
    let path_s = path.to_string_lossy();
    let link = read_shortcut(&path_s)?;
    let link_name = file_stem(&path_s);

    let target = link
        .target
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(expand_env);
    let target = target.as_deref();
    let aumid = link
        .app_user_model_id
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty());
    let apps_folder_name = aumid.and_then(apps_folder_display_name);
    let plan = plan_shortcut(target.is_some(), aumid, apps_folder_name.is_some());
    log::debug!(
        "{path_s}: target {target:?}, AUMID {aumid:?} (in AppsFolder: {}) -> {plan:?}",
        apps_folder_name.is_some()
    );
    if plan.id == ShortcutSource::Link {
        log::debug!(
            "{path_s}: shortcut has neither a target path nor an AUMID; pinning the .lnk itself"
        );
    }

    // Each arm needs the fact its source stands for; the plan only names a
    // source whose fact is present, and the `_` arms keep that an invariant
    // rather than a panic.
    let id = match (plan.id, aumid, target) {
        (ShortcutSource::AppUserModelId, Some(a), _) => AppId::for_app_user_model_id(a),
        (ShortcutSource::Target, _, Some(t)) => AppId::for_exe(Path::new(t)),
        _ => AppId::for_exe(path),
    };
    let display_name = match (plan.name, apps_folder_name, target) {
        (ShortcutSource::AppUserModelId, Some(name), _) => name,
        (ShortcutSource::Target, _, Some(t)) => {
            let target_stem = file_stem(t);
            first_display_name([
                exe_display_name(t).as_deref(),
                Some(&link_name),
                Some(&target_stem),
            ])
            .unwrap_or_else(|| PLACEHOLDER_DISPLAY_NAME.to_owned())
        }
        _ => clean_display_name(&link_name).unwrap_or_else(|| PLACEHOLDER_DISPLAY_NAME.to_owned()),
    };
    let icon_key = match (plan.icon, aumid, target) {
        (ShortcutSource::AppUserModelId, Some(a), _) => aumid_icon_key(a),
        (ShortcutSource::Target, _, Some(t)) => exe_icon_key(t),
        _ => file_icon_key(&path_s),
    };
    let launch = match (plan.launch, aumid, target) {
        (ShortcutSource::Target, _, Some(t)) => LaunchTarget::Exe {
            path: PathBuf::from(t),
            args: split_command_line(&link.arguments),
            working_dir: link
                .working_dir
                .as_deref()
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(|d| PathBuf::from(expand_env(d))),
        },
        (ShortcutSource::AppUserModelId, Some(a), _) => LaunchTarget::StoreApp {
            app_user_model_id: a.to_owned(),
        },
        _ => LaunchTarget::Shortcut {
            path: path.to_path_buf(),
        },
    };
    Ok(AppIdentity {
        id,
        display_name,
        icon_key: Some(icon_key),
        launch: Some(launch),
    })
}

/// What a `.lnk` file says, as plain strings.
#[derive(Debug, Default)]
struct ShortcutInfo {
    target: Option<String>,
    arguments: String,
    working_dir: Option<String>,
    app_user_model_id: Option<String>,
}

/// Load a `.lnk` through `IShellLinkW` + `IPersistFile`.
fn read_shortcut(path: &str) -> PlatformResult<ShortcutInfo> {
    // SAFETY: standard in-process activation of the shell link class.
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }?;
    let file: IPersistFile = link.cast()?;
    let wide = util::to_wide(path);
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    unsafe { file.Load(util::pcwstr(&wide), STGM_READ) }?;

    // Best effort: find a target that moved, without UI, without rewriting
    // the file, and within a bounded time (timeout in the high word).
    let resolve_flags = (SLR_NO_UI.0 | SLR_NOUPDATE.0) as u32 | (RESOLVE_TIMEOUT_MS << 16);
    // SAFETY: no owner window; flags forbid any UI.
    if let Err(e) = unsafe { link.Resolve(HWND::default(), resolve_flags) } {
        log::trace!("{path}: IShellLinkW::Resolve failed: {e}");
    }

    let mut info = ShortcutInfo::default();
    let mut buffer = vec![0u16; LINK_BUFFER];
    if is_advertised(&link) {
        // An advertised (MSI) shortcut: `GetPath` names the icon file the
        // installer cached under `C:\Windows\Installer\{ProductCode}\`, not
        // the program. MSI says which program; without it the `.lnk` itself
        // is pinned and launched, which lets the shell and MSI start it.
        info.target = msi_shortcut_target(path);
        match &info.target {
            Some(target) => log::debug!("{path}: advertised shortcut: MSI target {target}"),
            None => log::debug!("{path}: advertised shortcut: pinning the .lnk itself"),
        }
    // SAFETY: the wrapper passes the buffer length; no find-data requested.
    } else if unsafe { link.GetPath(&mut buffer, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32) }
        .is_ok()
    {
        info.target = Some(util::from_wide(&buffer)).filter(|s| !s.is_empty());
    }
    buffer.fill(0);
    // SAFETY: the wrapper passes the buffer length.
    if unsafe { link.GetArguments(&mut buffer) }.is_ok() {
        info.arguments = util::from_wide(&buffer);
    }
    buffer.fill(0);
    // SAFETY: the wrapper passes the buffer length.
    if unsafe { link.GetWorkingDirectory(&mut buffer) }.is_ok() {
        info.working_dir = Some(util::from_wide(&buffer)).filter(|s| !s.is_empty());
    }
    info.app_user_model_id = link_app_user_model_id(&link);
    Ok(info)
}

/// Whether the shortcut is an advertised (Windows Installer, "Darwin")
/// shortcut: `SLDF_HAS_DARWINID` in its data flags.
fn is_advertised(link: &IShellLinkW) -> bool {
    let Ok(data) = link.cast::<IShellLinkDataList>() else {
        return false;
    };
    // SAFETY: plain COM call on a live interface.
    unsafe { data.GetFlags() }.is_ok_and(|flags| flags & SLDF_HAS_DARWINID.0 as u32 != 0)
}

/// The installed program behind the advertised shortcut at `path`: the key
/// file of the component the shortcut names (`MsiGetShortcutTargetW`, then
/// `MsiGetComponentPathW`), when that component is installed locally and
/// the file exists. `None` otherwise (Windows Installer does not know the
/// shortcut, or the component is advertised but not installed). Neither
/// call installs, repairs or shows anything.
fn msi_shortcut_target(path: &str) -> Option<String> {
    const GUID_BUFFER: usize = MAX_GUID_CHARS as usize + 1;
    const FEATURE_BUFFER: usize = MAX_FEATURE_CHARS as usize + 1;
    let path_w = util::to_wide(path);
    let mut product = [0u16; GUID_BUFFER];
    let mut feature = [0u16; FEATURE_BUFFER];
    let mut component = [0u16; GUID_BUFFER];
    // SAFETY: `path_w` is NUL-terminated and outlives the call; each buffer
    // holds the documented maximum (38 characters) plus the NUL.
    let status = unsafe {
        MsiGetShortcutTargetW(
            util::pcwstr(&path_w),
            Some(PWSTR(product.as_mut_ptr())),
            Some(PWSTR(feature.as_mut_ptr())),
            Some(PWSTR(component.as_mut_ptr())),
        )
    };
    if status != ERROR_SUCCESS.0 {
        log::debug!("{path}: MsiGetShortcutTargetW failed ({status})");
        return None;
    }
    let mut buffer = vec![0u16; LINK_BUFFER];
    let mut len = LINK_BUFFER as u32;
    // SAFETY: the codes were written NUL-terminated by the call above;
    // `buffer` holds `len` characters, which the call is told.
    let state = unsafe {
        MsiGetComponentPathW(
            util::pcwstr(&product),
            util::pcwstr(&component),
            Some(PWSTR(buffer.as_mut_ptr())),
            Some(&mut len),
        )
    };
    if state != INSTALLSTATE_LOCAL {
        log::debug!(
            "{path}: its component is not installed locally (state {})",
            state.0
        );
        return None;
    }
    let target = util::from_wide(&buffer);
    (!target.is_empty() && Path::new(&target).is_file()).then_some(target)
}

/// `System.AppUserModel.ID` from the shortcut's property store, if set.
fn link_app_user_model_id(link: &IShellLinkW) -> Option<String> {
    let store: IPropertyStore = link.cast().ok()?;
    util::property_store_string(&store, &PKEY_AppUserModel_ID)
}

/// Display name of `shell:AppsFolder\<aumid>`; `None` when the shell does
/// not know the AUMID (or names it `.exe`-only, which no name may be).
fn apps_folder_display_name(aumid: &str) -> Option<String> {
    let name = util::to_wide(&apps_folder_parsing_name(aumid));
    // SAFETY: `name` is NUL-terminated and outlives the call; no bind context.
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(util::pcwstr(&name), None) }
        .inspect_err(|e| log::trace!("AppsFolder item for {aumid:?} not found: {e}"))
        .ok()?;
    // SAFETY: plain COM call; the returned string is CoTaskMem-allocated and
    // freed by the guard.
    let display = unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) }.ok()?;
    let display = CoTaskWideString::new(display)?.to_string_lossy();
    clean_display_name(&display)
}

/// `FileDescription`, else `ProductName`, from the exe's version resource,
/// cleaned (some resources repeat the file name, `.exe` included).
fn exe_display_name(path: &str) -> Option<String> {
    let block = version_block(path)?;
    let declared = query_bytes(&block, w!(r"\VarFileInfo\Translation"))
        .map(|b| translations(&b))
        .unwrap_or_default();
    for field in VERSION_NAME_FIELDS {
        for (lang, codepage) in translation_candidates(&declared) {
            let sub_block = util::to_wide(&version_string_path(lang, codepage, field));
            if let Some(value) =
                query_string(&block, util::pcwstr(&sub_block)).and_then(|v| clean_display_name(&v))
            {
                return Some(value);
            }
        }
    }
    None
}

/// The whole `VS_VERSIONINFO` block of a file, or `None` if it has none.
fn version_block(path: &str) -> Option<Vec<u8>> {
    let wide = util::to_wide(path);
    // SAFETY: NUL-terminated path; no handle output requested.
    let size = unsafe { GetFileVersionInfoSizeW(util::pcwstr(&wide), None) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0u8; size as usize];
    // SAFETY: `block` holds exactly `size` bytes as reported by the size query.
    unsafe { GetFileVersionInfoW(util::pcwstr(&wide), None, size, block.as_mut_ptr().cast()) }
        .inspect_err(|e| log::trace!("{path}: GetFileVersionInfoW failed: {e}"))
        .ok()?;
    Some(block)
}

/// Pointer and length (unit depends on the sub-block) of a value inside a
/// version block. The pointer refers into `block`.
fn query_value(block: &[u8], sub_block: PCWSTR) -> Option<(*const core::ffi::c_void, usize)> {
    let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut len: u32 = 0;
    // SAFETY: `block` is a complete version block; `sub_block` is a
    // NUL-terminated string that outlives the call; `ptr`/`len` are valid
    // out-pointers.
    let ok = unsafe { VerQueryValueW(block.as_ptr().cast(), sub_block, &mut ptr, &mut len) };
    if !ok.as_bool() || ptr.is_null() || len == 0 {
        return None;
    }
    let offset = (ptr as usize).checked_sub(block.as_ptr() as usize)?;
    (offset < block.len()).then_some((ptr.cast_const(), len as usize))
}

/// A binary value (e.g. the translation table), copied out of the block.
fn query_bytes(block: &[u8], sub_block: PCWSTR) -> Option<Vec<u8>> {
    let (ptr, len) = query_value(block, sub_block)?;
    let offset = ptr as usize - block.as_ptr() as usize;
    let end = offset.checked_add(len)?;
    block.get(offset..end).map(<[u8]>::to_vec)
}

/// A string value (`len` is in UTF-16 units, NUL included or not).
fn query_string(block: &[u8], sub_block: PCWSTR) -> Option<String> {
    let (ptr, len) = query_value(block, sub_block)?;
    let offset = ptr as usize - block.as_ptr() as usize;
    Some(version_string_value(block.get(offset..)?, len))
}

// ----- Executables under WindowsApps ------------------------------------------

/// At most this many installed packages of one family are looked at (a user
/// has one version of a package; resource packages share the family).
const MAX_FAMILY_PACKAGES: usize = 16;
/// Upper bound for the buffers the package APIs ask for, in bytes.
const MAX_PACKAGE_BUFFER_BYTES: u32 = 64 * 1024;
/// At most this many applications are read from one manifest.
const MAX_MANIFEST_APPLICATIONS: usize = 64;

/// The AppUserModelID of the packaged application that the executable at
/// `exe` (a path under `WindowsApps`) belongs to, or `None` when the package
/// is not installed for this user, has no application, or the choice is not
/// certain (`rules::identity::choose_package_app`); each `None` is logged at
/// debug. The directory in the path may name a version that has since been
/// updated away, so the package's family is looked up and whatever version
/// of it is installed now answers. Local calls only: nothing here waits on
/// another process.
fn packaged_app_aumid(exe: &PackagedExe<'_>) -> Option<String> {
    let full_name = exe.package_full_name;
    let family = package_family_name(full_name)
        .inspect_err(|e| log::debug!("{full_name}: PackageFamilyNameFromFullName failed: {e}"))
        .ok()?;
    let installed = match packages_of_family(&family) {
        Ok(names) if names.is_empty() => {
            log::debug!("{full_name}: no package of family {family} is installed for this user");
            return None;
        }
        Ok(names) => names,
        Err(e) => {
            log::debug!("{family}: GetPackagesByPackageFamily failed: {e}; trying {full_name}");
            vec![full_name.to_owned()]
        }
    };

    let mut apps: Vec<String> = Vec::new();
    for name in &installed {
        // The installed name carries the package's own spelling; the one in
        // the path may have been lower-cased along the way.
        let own_family = package_family_name(name).unwrap_or_else(|_| family.clone());
        match package_application_ids(name) {
            Ok(ids) => {
                for aumid in ids
                    .iter()
                    .filter_map(|id| package_app_aumid(&own_family, id))
                {
                    if !apps.contains(&aumid) {
                        apps.push(aumid);
                    }
                }
            }
            Err(e) => log::debug!("{name}: reading the application ids failed: {e}"),
        }
    }
    if apps.is_empty() {
        log::debug!("{full_name}: the installed package {installed:?} has no application");
        return None;
    }
    // The manifest only matters when there is a choice to make.
    let executables: Vec<(String, String)> = if apps.len() > 1 {
        installed
            .iter()
            .flat_map(|name| {
                manifest_executables(name)
                    .inspect_err(|e| log::debug!("{name}: reading AppxManifest.xml failed: {e}"))
                    .unwrap_or_default()
            })
            .collect()
    } else {
        Vec::new()
    };
    // The AppsFolder also resolves entries the Start menu does not list
    // (measured: `Claude_pzs8sxrjxfjjc!SshProxy` answers "Claude"), so this
    // tie-break rarely decides; the manifest does the real work.
    let chosen = choose_package_app(&apps, &executables, exe.relative_path, |aumid| {
        apps_folder_display_name(aumid).is_some()
    });
    if chosen.is_none() {
        log::debug!(
            "{full_name}: cannot tell which of {apps:?} runs {:?} (manifest executables {executables:?})",
            exe.relative_path
        );
    }
    chosen.map(str::to_owned)
}

/// `PackageFamilyNameFromFullName`: computed from the name alone, so it
/// works for a version that is no longer installed.
fn package_family_name(full_name: &str) -> PlatformResult<String> {
    let wide = util::to_wide(full_name);
    let mut buffer = vec![0u16; PACKAGE_FAMILY_NAME_MAX_LENGTH as usize + 1];
    let mut len = buffer.len() as u32;
    // SAFETY: `wide` is NUL-terminated; `buffer` is writable for `len` UTF-16
    // units and `len` is a live in/out pointer.
    unsafe {
        PackageFamilyNameFromFullName(
            util::pcwstr(&wide),
            &mut len,
            Some(PWSTR(buffer.as_mut_ptr())),
        )
    }
    .ok()?;
    Ok(util::from_wide(&buffer))
}

/// Full names of the packages of `family` installed for the current user
/// (`GetPackagesByPackageFamily`); empty when there is none.
fn packages_of_family(family: &str) -> PlatformResult<Vec<String>> {
    let wide = util::to_wide(family);
    let mut count = 0u32;
    let mut len = 0u32;
    // SAFETY: size query: no output arrays, `count` and `len` are live
    // out-pointers, `wide` is NUL-terminated.
    let err = unsafe {
        GetPackagesByPackageFamily(util::pcwstr(&wide), &mut count, None, &mut len, None)
    };
    if count == 0 && (err.is_ok() || err == ERROR_INSUFFICIENT_BUFFER) {
        return Ok(Vec::new());
    }
    if err != ERROR_INSUFFICIENT_BUFFER {
        return Err(windows::core::Error::from(err).into());
    }
    if count as usize > MAX_FAMILY_PACKAGES || len.saturating_mul(2) > MAX_PACKAGE_BUFFER_BYTES {
        return Err(PlatformError::Other(format!(
            "{family}: {count} packages in {len} UTF-16 units is more than expected"
        )));
    }
    let mut names = vec![PWSTR::null(); count as usize];
    let mut buffer = vec![0u16; len as usize];
    // SAFETY: `names` holds `count` pointers and `buffer` `len` units, the
    // sizes the query asked for; both outlive the call.
    unsafe {
        GetPackagesByPackageFamily(
            util::pcwstr(&wide),
            &mut count,
            Some(names.as_mut_ptr()),
            &mut len,
            Some(PWSTR(buffer.as_mut_ptr())),
        )
    }
    .ok()?;
    let addresses: Vec<usize> = names
        .iter()
        .take(count as usize)
        .map(|p| p.0 as usize)
        .collect();
    Ok(strings_at(&buffer, buffer.as_ptr() as usize, &addresses))
}

/// A package-information reference from `OpenPackageInfoByFullName`,
/// closed on drop.
struct PackageInfo(*mut _PACKAGE_INFO_REFERENCE);

impl PackageInfo {
    fn open(full_name: &str) -> PlatformResult<Self> {
        let wide = util::to_wide(full_name);
        let mut reference: *mut _PACKAGE_INFO_REFERENCE = std::ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated, `reference` is a live out-pointer
        // and the reserved argument is 0.
        unsafe { OpenPackageInfoByFullName(util::pcwstr(&wide), None, &mut reference) }.ok()?;
        if reference.is_null() {
            return Err(PlatformError::Other(format!(
                "{full_name}: OpenPackageInfoByFullName returned no reference"
            )));
        }
        Ok(Self(reference))
    }
}

impl Drop for PackageInfo {
    fn drop(&mut self) {
        // SAFETY: the reference came from a successful OpenPackageInfoByFullName
        // and is closed exactly once, here.
        let err = unsafe { ClosePackageInfo(self.0) };
        if err.is_err() {
            log::debug!("ClosePackageInfo failed: {err:?}");
        }
    }
}

/// The application ids of an installed package (`GetPackageApplicationIds`:
/// full AUMIDs, see `rules::identity::package_app_aumid`).
fn package_application_ids(full_name: &str) -> PlatformResult<Vec<String>> {
    let info = PackageInfo::open(full_name)?;
    let mut len = 0u32;
    let mut count = 0u32;
    // SAFETY: size query with no buffer; `len` and `count` are live
    // out-pointers; `info` is open.
    let err = unsafe { GetPackageApplicationIds(info.0, &mut len, None, Some(&mut count)) };
    if err.is_ok() {
        // Nothing to copy: a package without applications.
        return Ok(Vec::new());
    }
    if err != ERROR_INSUFFICIENT_BUFFER {
        return Err(windows::core::Error::from(err).into());
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    if len > MAX_PACKAGE_BUFFER_BYTES {
        return Err(PlatformError::Other(format!(
            "{full_name}: {len} bytes of application ids is more than expected"
        )));
    }
    // The API writes a table of `count` pointers at the start of the buffer
    // and the strings after it: allocate pointer-sized units so the table is
    // aligned.
    let unit = std::mem::size_of::<usize>();
    let mut buffer = vec![0usize; (len as usize).div_ceil(unit)];
    let mut len = (buffer.len() * unit) as u32;
    // SAFETY: `buffer` is writable for `len` bytes and outlives the call;
    // `info` is open.
    unsafe {
        GetPackageApplicationIds(
            info.0,
            &mut len,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut count),
        )
    }
    .ok()?;
    let addresses: Vec<usize> = buffer.iter().take(count as usize).copied().collect();
    // The same bytes as UTF-16 units, read by offset (never through the
    // pointers themselves).
    let bytes: Vec<u8> = buffer.iter().flat_map(|w| w.to_ne_bytes()).collect();
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .collect();
    Ok(strings_at(&units, buffer.as_ptr() as usize, &addresses))
}

/// Where an installed package lives (`GetPackagePathByFullName`).
fn package_path(full_name: &str) -> PlatformResult<String> {
    let wide = util::to_wide(full_name);
    let mut len = 0u32;
    // SAFETY: size query with no buffer; `len` is a live out-pointer.
    let err = unsafe { GetPackagePathByFullName(util::pcwstr(&wide), &mut len, None) };
    if err != ERROR_INSUFFICIENT_BUFFER {
        return Err(windows::core::Error::from(err).into());
    }
    if len == 0 || len.saturating_mul(2) > MAX_PACKAGE_BUFFER_BYTES {
        return Err(PlatformError::Other(format!(
            "{full_name}: a package path of {len} UTF-16 units"
        )));
    }
    let mut buffer = vec![0u16; len as usize];
    // SAFETY: `buffer` is writable for `len` UTF-16 units, the size the
    // query asked for.
    unsafe {
        GetPackagePathByFullName(
            util::pcwstr(&wide),
            &mut len,
            Some(PWSTR(buffer.as_mut_ptr())),
        )
    }
    .ok()?;
    Ok(util::from_wide(&buffer))
}

/// `(AUMID, Executable)` of every application in an installed package's
/// `AppxManifest.xml`, read with the OS's own manifest reader. Applications
/// without an `Executable` are left out. The caller holds a COM apartment
/// (`identify_target` does).
fn manifest_executables(full_name: &str) -> PlatformResult<Vec<(String, String)>> {
    let root = package_path(full_name)?;
    let manifest = format!(r"{}\AppxManifest.xml", root.trim_end_matches(['\\', '/']));
    let wide = util::to_wide(&manifest);
    // SAFETY: `wide` is NUL-terminated; read-only, nothing is created, no
    // template stream.
    let stream = unsafe {
        SHCreateStreamOnFileEx(
            util::pcwstr(&wide),
            (STGM_READ | STGM_SHARE_DENY_NONE).0,
            0,
            false,
            None,
        )
    }?;
    // SAFETY: standard in-process activation of the Appx factory.
    let factory: IAppxFactory =
        unsafe { CoCreateInstance(&AppxFactory, None, CLSCTX_INPROC_SERVER) }?;
    // SAFETY: plain COM calls on live interfaces; every returned string is
    // CoTaskMem-allocated and freed by its guard.
    let reader = unsafe { factory.CreateManifestReader(&stream) }?;
    // SAFETY: as above.
    let applications = unsafe { reader.GetApplications() }?;
    let mut out = Vec::new();
    // SAFETY: as above.
    let mut has_current = unsafe { applications.GetHasCurrent() }?.as_bool();
    while has_current && out.len() < MAX_MANIFEST_APPLICATIONS {
        // SAFETY: as above.
        let application = unsafe { applications.GetCurrent() }?;
        // SAFETY: as above.
        let aumid = unsafe { application.GetAppUserModelId() }
            .ok()
            .and_then(CoTaskWideString::new)
            .map(|s| s.to_string_lossy());
        // SAFETY: as above.
        let executable = unsafe { application.GetStringValue(w!("Executable")) }
            .ok()
            .and_then(CoTaskWideString::new)
            .map(|s| s.to_string_lossy());
        if let (Some(aumid), Some(executable)) = (aumid, executable)
            && !executable.trim().is_empty()
        {
            out.push((aumid, executable));
        }
        // SAFETY: as above.
        has_current = unsafe { applications.MoveNext() }?.as_bool();
    }
    Ok(out)
}

/// Expand `%VAR%` references the way the shell does (`ExpandEnvironmentStringsW`).
fn expand_env(value: &str) -> String {
    if !needs_env_expansion(value) {
        return value.to_owned();
    }
    let wide = util::to_wide(value);
    let mut buffer = vec![0u16; LINK_BUFFER];
    // SAFETY: NUL-terminated input; the wrapper passes the buffer length.
    let needed = unsafe { ExpandEnvironmentStringsW(util::pcwstr(&wide), Some(&mut buffer)) };
    if needed == 0 {
        return value.to_owned();
    }
    if needed as usize > buffer.len() {
        buffer = vec![0u16; needed as usize];
        // SAFETY: same as above, with a buffer of the size the OS asked for.
        let again = unsafe { ExpandEnvironmentStringsW(util::pcwstr(&wide), Some(&mut buffer)) };
        if again == 0 || again as usize > buffer.len() {
            return value.to_owned();
        }
    }
    util::from_wide(&buffer)
}

/// These run on Windows hosts only (this module is `cfg(windows)`). The
/// first two need nothing installed; the third proves the whole lookup chain
/// on a machine with Windows Terminal (any version) and says so when it
/// finds none.
#[cfg(test)]
mod tests {
    use super::*;

    const TERMINAL_FAMILY: &str = "Microsoft.WindowsTerminal_8wekyb3d8bbwe";
    /// A version no machine has: the pin of a Terminal that was updated since.
    const STALE_TERMINAL_EXE: &str = r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.0.0.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe";

    fn exe(path: &str) -> LaunchTarget {
        LaunchTarget::Exe {
            path: PathBuf::from(path),
            args: Vec::new(),
            working_dir: None,
        }
    }

    #[test]
    fn family_names_are_computed_from_the_full_name_alone() {
        assert_eq!(
            package_family_name("Microsoft.WindowsTerminal_1.0.0.0_x64__8wekyb3d8bbwe").unwrap(),
            TERMINAL_FAMILY
        );
        assert!(package_family_name("not a package").is_err());
    }

    #[test]
    fn an_exe_of_a_package_that_is_not_installed_keeps_its_exe_identity() {
        let path = r"C:\Program Files\WindowsApps\Stem.Test.NotInstalled_1.0.0.0_x64__0000000000000\app.exe";
        assert_eq!(
            packages_of_family("Stem.Test.NotInstalled_0000000000000").unwrap(),
            Vec::<String>::new()
        );
        assert!(PackageInfo::open("Stem.Test.NotInstalled_1.0.0.0_x64__0000000000000").is_err());
        let identity = identify_target(&exe(path)).unwrap();
        assert_eq!(identity.id, AppId::for_exe(Path::new(path)));
        assert_eq!(identity.display_name, "app");
        assert_eq!(identity.launch, Some(exe(path)));
    }

    #[test]
    fn a_stale_terminal_exe_is_identified_as_the_installed_terminal() {
        let installed = packages_of_family(TERMINAL_FAMILY).unwrap();
        let Some(current) = installed.first() else {
            eprintln!("Windows Terminal is not installed on this machine; nothing to check");
            return;
        };
        let aumid = format!("{TERMINAL_FAMILY}!App");
        assert_eq!(
            package_application_ids(current).unwrap(),
            vec![aumid.clone()]
        );
        // The manifest is not needed for one application, but it must read.
        let executables = {
            let _com = util::ComApartment::sta();
            manifest_executables(current).unwrap()
        };
        assert!(
            executables.contains(&(aumid.clone(), "WindowsTerminal.exe".to_owned())),
            "{executables:?}"
        );

        // The same identity a running Terminal window gets, whatever
        // the spelling of the path.
        for path in [
            STALE_TERMINAL_EXE.to_owned(),
            STALE_TERMINAL_EXE.to_lowercase(),
        ] {
            let identity = identify_target(&exe(&path)).unwrap();
            assert_eq!(identity.id, AppId::for_app_user_model_id(&aumid), "{path}");
            assert_eq!(identity.icon_key, Some(aumid_icon_key(&aumid)));
            assert_eq!(
                identity.launch,
                Some(LaunchTarget::StoreApp {
                    app_user_model_id: aumid.clone()
                })
            );
            assert_ne!(identity.display_name, "Windows Terminal Host");
            assert!(!identity.display_name.to_lowercase().ends_with(".exe"));
        }
    }

    /// The Microsoft Store package declares three applications, one per
    /// executable: the manifest picks the one that runs `WinStore.App.exe`.
    #[test]
    fn the_manifest_tells_the_applications_of_a_package_apart() {
        let family = "Microsoft.WindowsStore_8wekyb3d8bbwe";
        if packages_of_family(family).unwrap().is_empty() {
            eprintln!("the Microsoft Store is not installed on this machine; nothing to check");
            return;
        }
        let path = r"C:\Program Files\WindowsApps\Microsoft.WindowsStore_1.0.0.0_x64__8wekyb3d8bbwe\WinStore.App.exe";
        let identity = identify_target(&exe(path)).unwrap();
        assert_eq!(
            identity.id,
            AppId::for_app_user_model_id(&format!("{family}!App"))
        );
    }
}
