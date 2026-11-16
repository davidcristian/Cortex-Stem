//! Menu item ids and their dispatch.

use std::sync::Arc;

use stem_core::{AppId, ShellAction, WindowAction, WindowId};
use stem_service::{DockService, ServiceResult};
use tauri::menu::MenuEvent;
use tauri::{AppHandle, Manager};

use crate::AppService;

/// Prefix of every menu item id this module creates.
pub(super) const ID_PREFIX: &str = "ctx:";

/// What a menu entry does, encoded in and decoded from its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MenuAction {
    Activate(WindowId),
    Launch(AppId),
    Pin(AppId),
    Unpin(AppId),
    /// Stop showing the app on every dock.
    Hide(AppId),
    /// Every window of `app` that the dock window `dock` lists.
    CloseAll {
        dock: String,
        app: AppId,
    },
    Shell(ShellAction),
    /// Mute (`true`) or unmute the default output device (the
    /// volume item).
    Mute(bool),
    Settings,
    Quit,
}

impl MenuAction {
    pub(super) fn id(&self) -> String {
        match self {
            Self::Activate(id) => format!("{ID_PREFIX}activate:{}", id.0),
            Self::Launch(app) => format!("{ID_PREFIX}launch:{}", app.as_str()),
            Self::Pin(app) => format!("{ID_PREFIX}pin:{}", app.as_str()),
            Self::Unpin(app) => format!("{ID_PREFIX}unpin:{}", app.as_str()),
            Self::Hide(app) => format!("{ID_PREFIX}hide:{}", app.as_str()),
            Self::CloseAll { dock, app } => {
                format!("{ID_PREFIX}close-all:{dock}:{}", app.as_str())
            }
            Self::Shell(action) => format!("{ID_PREFIX}shell:{}", shell_action_name(*action)),
            Self::Mute(true) => format!("{ID_PREFIX}mute:on"),
            Self::Mute(false) => format!("{ID_PREFIX}mute:off"),
            Self::Settings => format!("{ID_PREFIX}settings"),
            Self::Quit => format!("{ID_PREFIX}quit"),
        }
    }

    /// `None` for ids that are not ours or are malformed.
    pub(super) fn parse(id: &str) -> Option<Self> {
        let rest = id.strip_prefix(ID_PREFIX)?;
        match rest {
            "settings" => return Some(Self::Settings),
            "quit" => return Some(Self::Quit),
            _ => {}
        }
        // App ids contain `:` themselves (`exe:c:/...`): split once only.
        let (verb, arg) = rest.split_once(':')?;
        let app = || (!arg.is_empty()).then(|| AppId(arg.to_owned()));
        match verb {
            "activate" => arg.parse::<u64>().ok().map(|n| Self::Activate(WindowId(n))),
            "launch" => app().map(Self::Launch),
            "pin" => app().map(Self::Pin),
            "unpin" => app().map(Self::Unpin),
            "hide" => app().map(Self::Hide),
            // The dock label first: it has no `:`, the app id may.
            "close-all" => arg.split_once(':').and_then(|(dock, app)| {
                (!dock.is_empty() && !app.is_empty()).then(|| Self::CloseAll {
                    dock: dock.to_owned(),
                    app: AppId(app.to_owned()),
                })
            }),
            "shell" => parse_shell_action(arg).map(Self::Shell),
            "mute" => match arg {
                "on" => Some(Self::Mute(true)),
                "off" => Some(Self::Mute(false)),
                _ => None,
            },
            _ => None,
        }
    }
}

/// The serialised (camelCase) name of a shell action, e.g. `openStartMenu`.
fn shell_action_name(action: ShellAction) -> String {
    match serde_json::to_value(action) {
        Ok(serde_json::Value::String(name)) => name,
        // `ShellAction` is a unit enum: serde always yields a string.
        _ => format!("{action:?}"),
    }
}

fn parse_shell_action(name: &str) -> Option<ShellAction> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
}

/// The global menu event handler (every menu, the tray's included).
pub(super) fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().0.as_str();
    if !id.starts_with(ID_PREFIX) {
        // The tray's items; `tray.rs` handles them.
        return;
    }
    let Some(action) = MenuAction::parse(id) else {
        log::warn!("ignoring unknown context menu item {id:?}");
        return;
    };
    log::debug!("context menu: {action:?}");
    match action {
        MenuAction::Settings => {
            if let Err(e) = crate::windows::open_settings_window(app) {
                log::warn!("opening the settings window from a context menu failed: {e}");
            }
        }
        MenuAction::Quit => {
            log::info!("quit requested from a context menu");
            crate::request_quit(app);
        }
        action => {
            if supersedes_focus(&action) {
                crate::windows::forget_dock_focus(app, "a menu entry");
            }
            let Some(service) = app.try_state::<AppService>().map(|s| Arc::clone(&s.0)) else {
                log::warn!("context menu {action:?} ignored: the service is not running");
                return;
            };
            // Off the UI thread: these end in Win32 calls that may wait on
            // another process (activation tricks, ShellExecute).
            drop(tauri::async_runtime::spawn_blocking(move || {
                if let Err(e) = perform(&service, &action) {
                    log::warn!("context menu {action:?} failed: {e}");
                }
            }));
        }
    }
}

/// Whether choosing `action` supersedes the window the focus shortcut would
/// go back to (docs/ipc/keyboard-entry.md), as a click does: an
/// entry that activates, launches or acts on windows does; pinning,
/// unpinning and hiding only edit the settings, muting
/// touches no window, and the settings and quit entries act on Stem
/// itself, so those do not. Exhaustive, so that a new entry has to be
/// decided here.
pub(super) fn supersedes_focus(action: &MenuAction) -> bool {
    match action {
        MenuAction::Activate(_)
        | MenuAction::Launch(_)
        | MenuAction::CloseAll { .. }
        | MenuAction::Shell(_) => true,
        MenuAction::Pin(_)
        | MenuAction::Unpin(_)
        | MenuAction::Hide(_)
        | MenuAction::Mute(_)
        | MenuAction::Settings
        | MenuAction::Quit => false,
    }
}

fn perform(service: &DockService, action: &MenuAction) -> ServiceResult<()> {
    match action {
        MenuAction::Activate(id) => service.window_action(*id, WindowAction::Activate),
        MenuAction::Launch(app) => service.launch_app(app),
        MenuAction::Pin(app) => service.pin_app(app),
        MenuAction::Unpin(app) => service.unpin_app(app),
        MenuAction::Hide(app) => service.hide_app(app),
        MenuAction::CloseAll { dock, app } => close_all(service, dock, app),
        MenuAction::Shell(shell) => service.shell_action(*shell),
        MenuAction::Mute(muted) => service.set_volume_muted(*muted),
        // Handled on the main thread by `on_menu_event`.
        MenuAction::Settings | MenuAction::Quit => Ok(()),
    }
}

/// Close every window of the app that the dock window `dock` lists *now*
/// (the list may have changed since the menu was built): with a dock per
/// monitor and `showOnlyCurrentMonitor` on, only those on its monitor.
/// Every window is tried; the first error is returned.
fn close_all(service: &DockService, dock: &str, app_id: &AppId) -> ServiceResult<()> {
    let windows = service.windows_on_dock(app_id, dock);
    if windows.is_empty() {
        log::debug!("close all: {app_id:?} has no windows in {dock:?} any more");
    }
    let mut first_error = None;
    for id in windows {
        if let Err(e) = service.window_action(id, WindowAction::Close) {
            log::warn!("closing window {id:?} of {app_id:?} failed: {e}");
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(()), Err)
}
