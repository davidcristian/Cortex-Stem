//! Shell links: what a `.lnk` points at, and the identity it gets.

use std::path::{Path, PathBuf};

use stem_core::{AppId, AppIdentity, LaunchTarget};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::ApplicationInstallationAndServicing::{
    INSTALLSTATE_LOCAL, MAX_FEATURE_CHARS, MAX_GUID_CHARS, MsiGetComponentPathW,
    MsiGetShortcutTargetW,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ,
};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    IShellLinkDataList, IShellLinkW, SLDF_HAS_DARWINID, SLGP_RAWPATH, SLR_NO_UI, SLR_NOUPDATE,
    ShellLink,
};
use windows::core::{Interface, PWSTR};

use super::names::{apps_folder_display_name, exe_display_name};
use crate::PlatformResult;
use crate::rules::identity::{
    PLACEHOLDER_DISPLAY_NAME, ShortcutSource, aumid_icon_key, clean_display_name, exe_icon_key,
    file_icon_key, file_stem, first_display_name, needs_env_expansion, plan_shortcut,
    split_command_line,
};
use crate::windows::util;

/// Buffer size for `IShellLinkW` string getters (long paths included).
const LINK_BUFFER: usize = 2048;
/// `IShellLinkW::Resolve` budget in milliseconds (high word of the flags).
const RESOLVE_TIMEOUT_MS: u32 = 500;

/// Resolve a `.lnk` and identify what it points at. The decision (which of
/// the AUMID property, the target and the link itself supplies the id, the
/// name, the icon and the launch target) is `rules::identity::plan_shortcut`.
pub(super) fn identify_shortcut(path: &Path) -> PlatformResult<AppIdentity> {
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
