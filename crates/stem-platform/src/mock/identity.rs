//! The identities of the scripted windows and targets, shaped like the real
//! platform's.

use stem_core::{AppId, AppIdentity, IconKey, LaunchTarget, RawWindow};

use crate::rules;

/// The identity of a window, shaped like `windows::identity::identify_window`:
/// a window whose AppUserModelID the shell knows
/// ([`apps_folder_knows`]) is that app ([`identity_for_aumid`]), so
/// Calculator in an `ApplicationFrameHost` frame launches the Store app and
/// names no file, not the frame host. Any other window, a desktop program
/// with an AppUserModelID of its own included (its id stays `aumid:`), is
/// named after its exe's stem, else its title, and launches that exe.
pub(super) fn identity_for(raw: &RawWindow) -> AppIdentity {
    if let Some(aumid) = raw
        .app_user_model_id
        .as_deref()
        .map(str::trim)
        .filter(|aumid| apps_folder_knows(aumid))
    {
        return identity_for_aumid(aumid);
    }
    let id = stem_core::app_id_for(raw);
    let display_name = raw
        .exe_path
        .as_deref()
        .map(file_stem)
        .unwrap_or_else(|| raw.title.clone());
    let launch = raw.exe_path.clone().map(|path| LaunchTarget::Exe {
        path,
        args: vec![],
        working_dir: None,
    });
    AppIdentity {
        icon_key: Some(IconKey(format!("mock:{}", id.as_str()))),
        id,
        display_name,
        launch,
    }
}

/// The identity of a packaged app, shaped like the real platform's
/// `identity_for_aumid`: the `aumid:` id, a `StoreApp` launch by the
/// AppUserModelID and no file. The mock has no AppsFolder to name the app,
/// so its name is the one the real platform derives from the AppUserModelID
/// when the shell cannot name it (`rules::identity::aumid_fallback_name`:
/// `Microsoft.WindowsCalculator_8wekyb3d8bbwe!App` → `WindowsCalculator`),
/// and its icon key is the mock's (`mock:<id>`) instead of `aumid:`.
pub(super) fn identity_for_aumid(aumid: &str) -> AppIdentity {
    let aumid = aumid.trim();
    let id = AppId::for_app_user_model_id(aumid);
    AppIdentity {
        icon_key: Some(IconKey(format!("mock:{}", id.as_str()))),
        id,
        display_name: rules::identity::aumid_fallback_name(aumid),
        launch: Some(LaunchTarget::StoreApp {
            app_user_model_id: aumid.to_owned(),
        }),
    }
}

/// Whether the mock's shell knows `aumid` as an AppsFolder item.
/// The real platform asks `shell:AppsFolder\<aumid>`, which knows every
/// packaged app and those desktop programs whose shortcut registered their
/// AppUserModelID. The mock knows exactly the packaged form,
/// `<package family name>!<application id>` with the family
/// `<name>_<publisher id>` (`Microsoft.WindowsCalculator_8wekyb3d8bbwe!App`),
/// and no desktop program's own AppUserModelID (`308046B0AF4A39CB`,
/// `Chrome.Profile1`), which therefore takes the exe branch as an AUMID the
/// shell does not know does on Windows. The TS mirror is `appsFolderKnows`
/// in `src/lib/ipc/mock/identity.ts`.
pub(super) fn apps_folder_knows(aumid: &str) -> bool {
    let Some((family, app)) = aumid.trim().split_once('!') else {
        return false;
    };
    let Some((name, publisher)) = family.split_once('_') else {
        return false;
    };
    !name.trim().is_empty()
        && !publisher.is_empty()
        && publisher.chars().all(|c| c.is_ascii_alphanumeric())
        && !app.trim().is_empty()
}

/// File name without extension, treating both `/` and `\` as separators so
/// Windows-style paths behave the same in tests on any host.
pub(super) fn file_stem(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    let name = s.rsplit(['/', '\\']).next().unwrap_or(&s);
    match name.rfind('.') {
        Some(0) | None => name.to_owned(),
        Some(dot) => name[..dot].to_owned(),
    }
}
