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

mod alias;
mod names;
mod package;
mod package_info;
mod shortcut;

use std::path::{Path, PathBuf};

use stem_core::{AppId, AppIdentity, LaunchTarget, RawWindow};

use super::util;
use crate::PlatformResult;
use crate::rules::identity::{
    PLACEHOLDER_DISPLAY_NAME, aumid_fallback_name, aumid_icon_key, display_name_fallback,
    exe_icon_key, file_stem, first_display_name, has_extension, hwnd_icon_key, packaged_exe,
    parse_apps_folder,
};
use alias::alias_redirect_aumid;
use names::{apps_folder_display_name, exe_display_name};
use package::packaged_app_aumid;
use shortcut::identify_shortcut;

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

/// These run on Windows hosts only (this module is `cfg(windows)`). The
/// first two need nothing installed; the third proves the whole lookup chain
/// on a machine with Windows Terminal (any version) and says so when it
/// finds none.
#[cfg(test)]
mod tests;
