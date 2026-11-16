//! Menu contents: what each dock item's menu and the dock's own menu
//! list.

use stem_core::{AppId, DockItem, DockItemKind, Settings, ShellAction, VolumeStatus};
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::{AppHandle, Wry};

use super::actions::MenuAction;

/// Window titles longer than this (in characters) are cut and end with `…`.
pub(super) const MAX_TITLE_CHARS: usize = 60;

type Builder<'a> = MenuBuilder<'a, Wry, AppHandle>;

/// The menu for `item` as the dock window `dock` lists it, or `None` for
/// items that have none (separators). `volume` is the engine's last volume
/// reading, for the volume item's *Mute* or *Unmute*.
pub(super) fn item_menu(
    app: &AppHandle,
    item: &DockItem,
    settings: &Settings,
    dock: &str,
    volume: Option<VolumeStatus>,
) -> Result<Option<Menu<Wry>>, String> {
    let menu = MenuBuilder::new(app);
    let menu = match item.kind {
        DockItemKind::Separator => return Ok(None),
        DockItemKind::App => app_entries(app, menu, item, settings, dock)?,
        DockItemKind::Launcher => shell_entry(menu, ShellAction::OpenStartMenu, "Open Start menu"),
        DockItemKind::TaskView => shell_entry(menu, ShellAction::OpenTaskView, "Open Task View"),
        DockItemKind::ShowDesktop => {
            shell_entry(menu, ShellAction::ToggleShowDesktop, "Show desktop")
        }
        DockItemKind::QuickSettings => {
            shell_entry(menu, ShellAction::OpenQuickSettings, "Open Quick Settings")
        }
        DockItemKind::NotificationCenter => shell_entry(
            menu,
            ShellAction::OpenNotificationCenter,
            "Open Notification Center",
        ),
        DockItemKind::Volume => volume_entries(app, menu, volume)?,
        // The panel a click opens, then the Settings page behind it.
        DockItemKind::Battery => {
            let menu = shell_entry(menu, ShellAction::OpenQuickSettings, "Open Quick Settings");
            shell_entry(
                menu,
                ShellAction::OpenPowerSettings,
                "Power and battery settings",
            )
        }
        DockItemKind::Clock => {
            let menu = shell_entry(
                menu,
                ShellAction::OpenNotificationCenter,
                "Open Notification Center",
            );
            shell_entry(
                menu,
                ShellAction::OpenDateTimeSettings,
                "Date and time settings",
            )
        }
        DockItemKind::RecycleBin => {
            let menu = shell_entry(menu, ShellAction::OpenRecycleBin, "Open");
            shell_entry(menu, ShellAction::EmptyRecycleBin, "Empty Recycle Bin…")
        }
    };
    tail(menu.separator())
        .build()
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Right-click on the panel background: settings and quit only.
pub(super) fn dock_menu(app: &AppHandle) -> Result<Menu<Wry>, String> {
    MenuBuilder::new(app)
        .text(MenuAction::Settings.id(), "Stem settings…")
        .separator()
        .text(MenuAction::Quit.id(), "Quit Stem")
        .build()
        .map_err(|e| e.to_string())
}

/// The common tail of every item menu.
fn tail(menu: Builder<'_>) -> Builder<'_> {
    menu.text(MenuAction::Settings.id(), "Stem settings…")
        .text(MenuAction::Quit.id(), "Quit Stem")
}

fn shell_entry<'a>(menu: Builder<'a>, action: ShellAction, text: &str) -> Builder<'a> {
    menu.text(MenuAction::Shell(action).id(), text)
}

/// The volume item: *Mute*, or *Unmute* while the engine's last
/// reading says muted (disabled without an output device, to show the
/// entry is not missing), then the Settings app's sound page and its volume
/// mixer.
fn volume_entries<'a>(
    app: &AppHandle,
    menu: Builder<'a>,
    volume: Option<VolumeStatus>,
) -> Result<Builder<'a>, String> {
    let menu = match volume {
        Some(v) => menu.text(MenuAction::Mute(!v.muted).id(), mute_text(!v.muted)),
        None => {
            let mute = MenuItemBuilder::with_id(MenuAction::Mute(true).id(), mute_text(true))
                .enabled(false)
                .build(app)
                .map_err(|e| e.to_string())?;
            menu.item(&mute)
        }
    };
    let menu = shell_entry(menu, ShellAction::OpenSoundSettings, "Sound settings");
    Ok(shell_entry(
        menu,
        ShellAction::OpenVolumeMixer,
        "Volume mixer",
    ))
}

/// The text of the entry that mutes (`true`) or unmutes the output.
pub(super) fn mute_text(mute: bool) -> &'static str {
    if mute { "Mute" } else { "Unmute" }
}

/// An app item: its windows (activate), *New window*, *Pin* / *Unpin*,
/// *Hide from Stem* for an unpinned app, *Close all windows* (of
/// the dock window `dock`). `item` is the dock's view of it, so the windows
/// are the ones that dock lists.
fn app_entries<'a>(
    app: &AppHandle,
    mut menu: Builder<'a>,
    item: &DockItem,
    settings: &Settings,
    dock: &str,
) -> Result<Builder<'a>, String> {
    let app_id = item
        .app_id
        .as_ref()
        .ok_or_else(|| format!("app item {} has no app id", item.key))?;
    for window in &item.windows {
        menu = menu.text(
            MenuAction::Activate(window.id).id(),
            menu_text(&window.title),
        );
    }
    if !item.windows.is_empty() {
        menu = menu.separator();
    }
    let pinned = settings.is_pinned(app_id);
    let launchable = pinned || has_launch_target(app_id);
    if launchable {
        menu = menu.text(MenuAction::Launch(app_id.clone()).id(), "New window");
    }
    if pinned {
        menu = menu.text(MenuAction::Unpin(app_id.clone()).id(), "Unpin from Stem");
    } else if launchable {
        menu = menu.text(MenuAction::Pin(app_id.clone()).id(), "Pin to Stem");
    } else {
        // Pinning needs a launch target (`pin_app` would refuse); show why
        // the entry is missing rather than hiding it.
        let pin = MenuItemBuilder::with_id(MenuAction::Pin(app_id.clone()).id(), "Pin to Stem")
            .enabled(false)
            .build(app)
            .map_err(|e| e.to_string())?;
        menu = menu.item(&pin);
    }
    if !pinned {
        // Every unpinned item is a running app (the engine lists no other),
        // and hiding needs no launch target, so a `class:` app has it too. A
        // pinned app is never hidden (`hide_app` would refuse it). No
        // confirmation: *Show* on the settings page undoes it.
        menu = menu.text(MenuAction::Hide(app_id.clone()).id(), "Hide from Stem");
    }
    if !item.windows.is_empty() {
        menu = menu.text(
            MenuAction::CloseAll {
                dock: dock.to_owned(),
                app: app_id.clone(),
            }
            .id(),
            "Close all windows",
        );
    }
    Ok(menu)
}

/// Whether an *unpinned* app can be launched. The service decides for real
/// (`launch_app` fails with `NotLaunchable`), but it has no read-only
/// accessor for resolved identities, so this reads the fact off the id: the
/// platforms derive an `exe:` or `aumid:` id exactly when they could read
/// the exe path or the AppUserModelID, which is also what they build the
/// launch target from; a `class:` id means neither was readable
/// (`stem_core::app_id_for`, `identify_window`). Pinned apps always have a
/// target (the pin stores it).
pub(super) fn has_launch_target(app_id: &AppId) -> bool {
    let id = app_id.as_str();
    id.starts_with("exe:") || id.starts_with("aumid:")
}

/// Menu text for a window title: trimmed, cut to [`MAX_TITLE_CHARS`] with
/// an ellipsis, never empty, and with `&` escaped (`&` marks a mnemonic in
/// native menus; `&&` shows one).
pub(super) fn menu_text(title: &str) -> String {
    let title = title.trim();
    let title = if title.is_empty() {
        "(untitled)"
    } else {
        title
    };
    let mut text: String = title.chars().take(MAX_TITLE_CHARS).collect();
    if title.chars().count() > MAX_TITLE_CHARS {
        text.push('…');
    }
    text.replace('&', "&&")
}
