//! How a `.lnk` shortcut is identified, named, drawn and launched.

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
