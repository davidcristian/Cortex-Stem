//! Native context menus: the right-click menus of dock items and of
//! the panel background, shown with Tauri's `Menu::popup_at`.
//!
//! Flow: the UI reports *where* the user right-clicked (`show_item_menu`,
//! `show_dock_menu`; CSS px in the calling dock window's viewport). This
//! module builds a menu from the item as the calling dock lists it
//! (`DockService::dock_item`: with a dock per monitor and
//! `showOnlyCurrentMonitor` on, only the windows on that dock's monitor)
//! and the settings, pops it up at that point (scaled by the
//! window's scale factor), and, through the app-wide menu event handler that
//! [`init`] registers, dispatches the chosen entry to the service. Menus are
//! built fresh per call and never cached: window lists and pinned state
//! change under them.
//!
//! Item ids encode the action: `ctx:activate:<windowId>`,
//! `ctx:launch:<appId>`, `ctx:pin:<appId>`, `ctx:unpin:<appId>`,
//! `ctx:hide:<appId>`, `ctx:close-all:<dockLabel>:<appId>` (the
//! dock whose windows it closes; dock labels have no `:`, app ids may),
//! `ctx:shell:<shellAction>` (camelCase, as serialised), `ctx:settings`,
//! `ctx:quit`. Everything starts with `ctx:` because the tray's items
//! (`tray.rs`) share the same global event stream and must not collide; ids
//! without the prefix are not ours and are ignored.
//!
//! Threading: `popup_at` runs the modal menu loop on the main thread, so the
//! command blocks until the menu closes (as every Win32 context menu does;
//! the message loop keeps pumping). Service calls from the event handler hop
//! to a blocking thread so Win32 work (activation fallbacks, launching,
//! closing) never runs on the UI thread (docs/development/windows-rules.md).
//! *Stem settings…* and *Quit Stem* stay on the main thread: they are window
//! operations.
//!
//! Windows: `TrackPopupMenu` only dismisses on an outside click when the
//! owning window is the foreground window, which a `WS_EX_NOACTIVATE` dock
//! window never is on its own (the classic "menu does not close" bug). muda
//! calls `SetForegroundWindow` before `TrackPopupMenu`; the platform's
//! `finish_popup` adds the other half of the documented recipe
//! (`PostMessage(WM_NULL)` after the menu closes) and hands the foreground
//! back to the window that had it (`foreground_native_window`, captured
//! before the popup), so opening a menu does not leave the dock focused.
//! All of that Win32 lives in `stem-platform` (AGENTS.md §4).

use std::sync::Arc;

use stem_core::{AppId, DockItem, DockItemKind, Settings, ShellAction, WindowAction, WindowId};
use stem_service::{DockService, ServiceResult};
use tauri::menu::{ContextMenu, Menu, MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::{AppHandle, Manager, PhysicalPosition, State, WebviewWindow, Wry};

use crate::AppService;

/// Prefix of every menu item id this module creates.
const ID_PREFIX: &str = "ctx:";
/// Window titles longer than this (in characters) are cut and end with `…`.
const MAX_TITLE_CHARS: usize = 60;

type Builder<'a> = MenuBuilder<'a, Wry, AppHandle>;

/// Register the app-wide menu event handler. Call once from `setup`, before
/// any menu can be shown. (Never fails today; the signature leaves room for
/// setup work that can.)
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    app.on_menu_event(on_menu_event);
    log::debug!("context menu handler registered");
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands (docs/ipc/context-menus.md, v1.2)
// ---------------------------------------------------------------------------

/// Show the context menu of one dock item at `(x, y)` CSS px of the calling
/// dock window, with the windows that dock lists. Separators have no menu
/// (returns `Ok` without showing one); an unknown key rejects. Returns once
/// the menu has closed.
#[tauri::command]
pub(crate) fn show_item_menu(
    app: AppHandle,
    window: WebviewWindow,
    service: State<'_, AppService>,
    item_key: String,
    x: f64,
    y: f64,
) -> Result<(), String> {
    let args = (window.label().to_owned(), item_key.clone(), x, y);
    let shown = (|| {
        let point = popup_point(&window, x, y)?;
        let dock = window.label();
        let item = service
            .0
            .dock_item(&item_key, dock)
            .ok_or_else(|| format!("unknown dock item: {item_key}"))?;
        let settings = service.0.settings();
        let Some(menu) = item_menu(&app, &item, &settings, dock)? else {
            log::debug!("no context menu for {item_key}");
            return Ok(());
        };
        popup(&service.0, &window, &menu, point)
    })();
    checked("show_item_menu", &args, shown)
}

/// Show the dock's own menu (right-click on the panel background).
#[tauri::command]
pub(crate) fn show_dock_menu(
    app: AppHandle,
    window: WebviewWindow,
    service: State<'_, AppService>,
    x: f64,
    y: f64,
) -> Result<(), String> {
    let args = (window.label().to_owned(), x, y);
    let shown = (|| {
        let point = popup_point(&window, x, y)?;
        let menu = dock_menu(&app)?;
        popup(&service.0, &window, &menu, point)
    })();
    checked("show_dock_menu", &args, shown)
}

/// Log a failed command with its arguments (as `commands.rs` does).
fn checked<T>(
    command: &str,
    args: &dyn std::fmt::Debug,
    result: Result<T, String>,
) -> Result<T, String> {
    result.map_err(|e| {
        log::warn!("{command}({args:?}) failed: {e}");
        e
    })
}

/// Validate the caller and convert the CSS point to physical pixels
/// relative to the window's top-left corner, which is what `popup_at` wants.
fn popup_point(window: &WebviewWindow, x: f64, y: f64) -> Result<PhysicalPosition<i32>, String> {
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return Err("only dock windows have context menus".to_owned());
    }
    if !(x.is_finite() && y.is_finite()) {
        return Err("x and y must be finite numbers".to_owned());
    }
    let scale = match window.scale_factor() {
        Ok(s) if s.is_finite() && s > 0.0 => s,
        Ok(s) => {
            log::warn!("window scale factor {s} is unusable; using 1.0 for the menu position");
            1.0
        }
        Err(e) => {
            log::warn!("window scale factor unavailable ({e}); using 1.0 for the menu position");
            1.0
        }
    };
    Ok(PhysicalPosition::new(
        (x * scale).round() as i32,
        (y * scale).round() as i32,
    ))
}

/// Pop the menu up and block until it closes; see the module docs for the
/// foreground handling around it.
fn popup(
    service: &DockService,
    window: &WebviewWindow,
    menu: &Menu<Wry>,
    point: PhysicalPosition<i32>,
) -> Result<(), String> {
    log::debug!(
        "context menu on {:?} at {}x{} physical",
        window.label(),
        point.x,
        point.y
    );
    let target = AsRef::<tauri::Webview>::as_ref(window).window();
    let platform = service.platform();
    let dock = crate::windows::native_handle(window);
    let previous = platform.foreground_native_window();
    let shown = menu.popup_at(target, point);
    if let Some(dock) = dock {
        platform.finish_popup(dock, previous);
    }
    shown.map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Menu contents
// ---------------------------------------------------------------------------

/// The menu for `item` as the dock window `dock` lists it, or `None` for
/// items that have none (separators).
fn item_menu(
    app: &AppHandle,
    item: &DockItem,
    settings: &Settings,
    dock: &str,
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
fn dock_menu(app: &AppHandle) -> Result<Menu<Wry>, String> {
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
fn has_launch_target(app_id: &AppId) -> bool {
    let id = app_id.as_str();
    id.starts_with("exe:") || id.starts_with("aumid:")
}

/// Menu text for a window title: trimmed, cut to [`MAX_TITLE_CHARS`] with
/// an ellipsis, never empty, and with `&` escaped (`&` marks a mnemonic in
/// native menus; `&&` shows one).
fn menu_text(title: &str) -> String {
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

// ---------------------------------------------------------------------------
// Menu item ids and their dispatch
// ---------------------------------------------------------------------------

/// What a menu entry does, encoded in and decoded from its id.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MenuAction {
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
    Settings,
    Quit,
}

impl MenuAction {
    fn id(&self) -> String {
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
            Self::Settings => format!("{ID_PREFIX}settings"),
            Self::Quit => format!("{ID_PREFIX}quit"),
        }
    }

    /// `None` for ids that are not ours or are malformed.
    fn parse(id: &str) -> Option<Self> {
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
fn on_menu_event(app: &AppHandle, event: MenuEvent) {
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
/// unpinning and hiding only edit the settings, and the settings
/// and quit entries act on Stem itself, so those do not. Exhaustive, so
/// that a new entry has to be decided here.
fn supersedes_focus(action: &MenuAction) -> bool {
    match action {
        MenuAction::Activate(_)
        | MenuAction::Launch(_)
        | MenuAction::CloseAll { .. }
        | MenuAction::Shell(_) => true,
        MenuAction::Pin(_)
        | MenuAction::Unpin(_)
        | MenuAction::Hide(_)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_round_trip_through_parse() {
        let actions = [
            MenuAction::Activate(WindowId(0x1_0042)),
            MenuAction::Launch(AppId("exe:c:/program files/app/app.exe".into())),
            MenuAction::Pin(AppId(
                "aumid:Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into(),
            )),
            MenuAction::Unpin(AppId("uri:ms-settings:".into())),
            MenuAction::Hide(AppId("exe:c:/windows/notepad.exe".into())),
            MenuAction::Hide(AppId("class:notepad".into())),
            MenuAction::CloseAll {
                dock: "dock-DISPLAY3".into(),
                app: AppId("class:notepad".into()),
            },
            MenuAction::CloseAll {
                dock: "dock".into(),
                app: AppId("exe:c:/windows/notepad.exe".into()),
            },
            MenuAction::Shell(ShellAction::EmptyRecycleBin),
            MenuAction::Shell(ShellAction::OpenQuickSettings),
            MenuAction::Shell(ShellAction::OpenNotificationCenter),
            MenuAction::Shell(ShellAction::OpenDateTimeSettings),
            MenuAction::Shell(ShellAction::OpenPowerSettings),
            MenuAction::Settings,
            MenuAction::Quit,
        ];
        for action in actions {
            let id = action.id();
            assert!(id.starts_with(ID_PREFIX), "{id}");
            assert_eq!(MenuAction::parse(&id), Some(action), "{id}");
        }
        assert_eq!(
            MenuAction::Shell(ShellAction::OpenStartMenu).id(),
            "ctx:shell:openStartMenu"
        );
        assert_eq!(
            MenuAction::Shell(ShellAction::OpenQuickSettings).id(),
            "ctx:shell:openQuickSettings"
        );
        assert_eq!(
            MenuAction::Shell(ShellAction::OpenNotificationCenter).id(),
            "ctx:shell:openNotificationCenter"
        );
        assert_eq!(
            MenuAction::Shell(ShellAction::OpenDateTimeSettings).id(),
            "ctx:shell:openDateTimeSettings"
        );
        assert_eq!(
            MenuAction::Shell(ShellAction::OpenPowerSettings).id(),
            "ctx:shell:openPowerSettings"
        );
        assert_eq!(MenuAction::Activate(WindowId(7)).id(), "ctx:activate:7");
        assert_eq!(
            MenuAction::Hide(AppId("exe:c:/windows/notepad.exe".into())).id(),
            "ctx:hide:exe:c:/windows/notepad.exe"
        );
        assert_eq!(
            MenuAction::CloseAll {
                dock: "dock-DISPLAY3".into(),
                app: AppId("exe:c:/windows/notepad.exe".into()),
            }
            .id(),
            "ctx:close-all:dock-DISPLAY3:exe:c:/windows/notepad.exe"
        );
    }

    #[test]
    fn foreign_and_malformed_ids_do_not_parse() {
        for id in [
            "settings",
            "quit",
            "ctx:",
            "ctx:nope",
            "ctx:activate:",
            "ctx:activate:x",
            "ctx:launch:",
            "ctx:hide:",
            "ctx:hide",
            "ctx:shell:OpenStartMenu",
            "ctx:shell:",
            "ctx:close-all",
            "ctx:close-all:dock",
            "ctx:close-all:dock:",
            "ctx:close-all::exe:c:/a.exe",
        ] {
            assert_eq!(MenuAction::parse(id), None, "{id}");
        }
    }

    #[test]
    fn only_entries_that_act_on_windows_supersede_the_focus_shortcut() {
        let notepad = || AppId("exe:c:/windows/notepad.exe".into());
        // They activate, launch or act on windows, as a click does.
        for action in [
            MenuAction::Activate(WindowId(0x1_0042)),
            MenuAction::Launch(notepad()),
            MenuAction::CloseAll {
                dock: "dock".into(),
                app: notepad(),
            },
            MenuAction::Shell(ShellAction::OpenStartMenu),
            MenuAction::Shell(ShellAction::EmptyRecycleBin),
        ] {
            assert!(supersedes_focus(&action), "{action:?}");
        }
        // Settings changes (Hide among them) and Stem's own entries:
        // Escape still gives the focus back to the window in front at the press.
        for action in [
            MenuAction::Pin(notepad()),
            MenuAction::Unpin(notepad()),
            MenuAction::Hide(notepad()),
            MenuAction::Settings,
            MenuAction::Quit,
        ] {
            assert!(!supersedes_focus(&action), "{action:?}");
        }
    }

    #[test]
    fn menu_text_truncates_escapes_and_never_goes_empty() {
        assert_eq!(menu_text("  Downloads  "), "Downloads");
        assert_eq!(menu_text(""), "(untitled)");
        assert_eq!(menu_text("Fish & Chips"), "Fish && Chips");
        let long = "x".repeat(MAX_TITLE_CHARS + 5);
        let text = menu_text(&long);
        assert_eq!(text.chars().count(), MAX_TITLE_CHARS + 1);
        assert!(text.ends_with('…'));
        // Exactly the limit: untouched (multi-byte characters count as one).
        let exact = "é".repeat(MAX_TITLE_CHARS);
        assert_eq!(menu_text(&exact), exact);
    }

    #[test]
    fn launchability_follows_the_id_namespace() {
        assert!(has_launch_target(&AppId(
            "exe:c:/windows/notepad.exe".into()
        )));
        assert!(has_launch_target(&AppId("aumid:Some.App!App".into())));
        assert!(!has_launch_target(&AppId("class:someclass".into())));
        assert!(!has_launch_target(&AppId("uri:ms-settings:".into())));
    }
}
