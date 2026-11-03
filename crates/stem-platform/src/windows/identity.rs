//! Application identity.
//!
//! Contract:
//! - [`identify_window`] takes the `RawWindow` facts already collected by
//!   `enumerate` and produces an [`AppIdentity`]: `id` via
//!   `stem_core::app_id_for`, `display_name` from the exe's `FileDescription`
//!   version resource (fallback: `ProductName`, then file stem, then window
//!   title), `icon_key` (`aumid:` for packaged apps, otherwise `exe:`), and a
//!   `launch` target (`StoreApp` for packaged apps, otherwise `Exe`).
//! - [`identify_target`] resolves what the user wants to pin: `.lnk` files are
//!   resolved through `IShellLinkW` (target path, arguments, working dir, icon
//!   location); `shell:AppsFolder\<aumid>` and `StoreApp` map to `aumid:`;
//!   plain executables map to `exe:`. The returned `id` **must** equal the id
//!   the engine would derive for that app's windows, or pins and running
//!   windows will not merge.
//! - Elevated/protected processes: `exe_path` may be `None`; degrade to the
//!   window title and class without failing.
//!
//! Decisions:
//! - A window with an AppUserModelID whose `shell:AppsFolder\<aumid>` item
//!   resolves (Store apps, and desktop apps that registered an AUMID such as
//!   browser profiles) gets its name from the shell, an `aumid:` icon and a
//!   `StoreApp` launch target (`launch` falls back to `ShellExecuteExW` for
//!   non-packaged AUMIDs). An AUMID the shell does not know falls back to the
//!   exe for name/icon/launch while keeping the AUMID-based id.
//! - Shortcuts keep the resolved exe + arguments as the launch target, not the
//!   `.lnk`, so the pinned id equals the running app's `exe:` id. A shortcut
//!   with no target but an AUMID property (Store app shortcuts) is a
//!   `StoreApp`. `.url` files are not shell links; they stay `Shortcut`
//!   targets with a `file:` icon.
//! - Pure helpers (parsing, fallbacks, command-line splitting) live in
//!   `rules::identity` and are unit-tested on every host.

use std::path::{Path, PathBuf};

use stem_core::{AppId, AppIdentity, LaunchTarget, RawWindow};
use windows::Win32::Foundation::HWND;
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ,
};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    IShellItem, IShellLinkW, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY, SLGP_RAWPATH,
    SLR_NO_UI, SLR_NOUPDATE, ShellLink,
};
use windows::core::{Interface, PCWSTR, w};

use super::util::{self, CoTaskWideString, PropVariantGuard};
use crate::PlatformResult;
use crate::rules::identity::{
    VERSION_NAME_FIELDS, apps_folder_parsing_name, aumid_fallback_name, aumid_icon_key,
    display_name_fallback, exe_icon_key, file_icon_key, file_stem, first_non_empty, has_extension,
    hwnd_icon_key, needs_env_expansion, parse_apps_folder, split_command_line,
    translation_candidates, translations, version_string_path,
};

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
        .or_else(|| Some(hwnd_icon_key(raw.id.0)));
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
    let display_name = first_non_empty([from_version.as_deref(), preferred_name])
        .unwrap_or_else(|| file_stem(path));
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

/// Resolve a `.lnk` and identify what it points at.
fn identify_shortcut(path: &Path) -> PlatformResult<AppIdentity> {
    let path_s = path.to_string_lossy();
    let link = read_shortcut(&path_s)?;
    let link_name = file_stem(&path_s);

    if let Some(target) = link
        .target
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        let target = expand_env(target);
        let working_dir = link
            .working_dir
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(|d| PathBuf::from(expand_env(d)));
        return Ok(identity_for_exe(
            &target,
            split_command_line(&link.arguments),
            working_dir,
            Some(&link_name),
        ));
    }
    if let Some(aumid) = link
        .app_user_model_id
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
    {
        return Ok(identity_for_aumid(aumid));
    }
    log::debug!(
        "{path_s}: shortcut has neither a target path nor an AUMID; pinning the .lnk itself"
    );
    Ok(AppIdentity {
        id: AppId::for_exe(path),
        display_name: link_name,
        icon_key: Some(file_icon_key(&path_s)),
        launch: Some(LaunchTarget::Shortcut {
            path: path.to_path_buf(),
        }),
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
    // SAFETY: the wrapper passes the buffer length; no find-data requested.
    if unsafe { link.GetPath(&mut buffer, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32) }.is_ok() {
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

/// `System.AppUserModel.ID` from the shortcut's property store, if set.
fn link_app_user_model_id(link: &IShellLinkW) -> Option<String> {
    let store: IPropertyStore = link.cast().ok()?;
    // SAFETY: the key is a static PROPERTYKEY; the returned PROPVARIANT is
    // owned by us and cleared by the guard.
    let value = unsafe { store.GetValue(&PKEY_AppUserModel_ID) }.ok()?;
    let value = PropVariantGuard::new(value);
    // SAFETY: `value` is a live PROPVARIANT; the returned string is
    // CoTaskMem-allocated and freed by the guard.
    let text = unsafe { PropVariantToStringAlloc(value.as_ptr()) }.ok()?;
    let text = CoTaskWideString::new(text)?.to_string_lossy();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Display name of `shell:AppsFolder\<aumid>`; `None` when the shell does
/// not know the AUMID.
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
    let display = display.trim();
    (!display.is_empty()).then(|| display.to_owned())
}

/// `FileDescription`, else `ProductName`, from the exe's version resource.
fn exe_display_name(path: &str) -> Option<String> {
    let block = version_block(path)?;
    let declared = query_bytes(&block, w!(r"\VarFileInfo\Translation"))
        .map(|b| translations(&b))
        .unwrap_or_default();
    for field in VERSION_NAME_FIELDS {
        for (lang, codepage) in translation_candidates(&declared) {
            let sub_block = util::to_wide(&version_string_path(lang, codepage, field));
            if let Some(value) = query_string(&block, util::pcwstr(&sub_block)) {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_owned());
                }
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
    let bytes = block.get(offset..)?;
    let units = len.min(bytes.len() / 2);
    let wide: Vec<u16> = bytes[..units * 2]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(util::from_wide(&wide))
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
