//! Native context menus: the right-click menus of dock items and of
//! the panel background, shown with Tauri's `Menu::popup_at`.
//!
//! Flow: the UI reports *where* the user right-clicked (`show_item_menu`,
//! `show_dock_menu`; CSS px in the calling dock window's viewport). This
//! module builds a menu from the service's current snapshot and settings,
//! pops it up at that point (scaled by the window's scale factor), and,
//! through the app-wide menu event handler that [`init`] registers,
//! dispatches the chosen entry to the service. Menus are built fresh per
//! call and never cached: window lists and pinned state change under them.
//!
//! Item ids encode the action: `ctx:activate:<windowId>`,
//! `ctx:launch:<appId>`, `ctx:pin:<appId>`, `ctx:unpin:<appId>`,
//! `ctx:close-all:<appId>`, `ctx:shell:<shellAction>` (camelCase, as
//! serialised), `ctx:settings`, `ctx:quit`. Everything starts with `ctx:`
//! because the tray's items (`tray.rs`) share the same global event stream
//! and must not collide; ids without the prefix are not ours and are ignored.
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
//! calls `SetForegroundWindow` before `TrackPopupMenu`; [`native`] adds the
//! other half of the documented recipe (`PostMessage(WM_NULL)` after the
//! menu closes) and hands the foreground back to the window that had it, so
//! opening a menu does not leave the dock focused.

use std::sync::Arc;

use stem_core::{
    AppId, DockItem, DockItemKind, DockState, Settings, ShellAction, WindowAction, WindowId,
};
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
/// dock window. Separators have no menu (returns `Ok` without showing one);
/// an unknown key rejects. Returns once the menu has closed.
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
        let state = service.0.state();
        let settings = service.0.settings();
        let Some(menu) = item_menu(&app, &state, &settings, &item_key)? else {
            log::debug!("no context menu for {item_key}");
            return Ok(());
        };
        popup(&window, &menu, point)
    })();
    checked("show_item_menu", &args, shown)
}

/// Show the dock's own menu (right-click on the panel background).
#[tauri::command]
pub(crate) fn show_dock_menu(
    app: AppHandle,
    window: WebviewWindow,
    x: f64,
    y: f64,
) -> Result<(), String> {
    let args = (window.label().to_owned(), x, y);
    let shown = (|| {
        let point = popup_point(&window, x, y)?;
        let menu = dock_menu(&app)?;
        popup(&window, &menu, point)
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
    let focus = native::before_popup(window);
    let shown = menu.popup_at(target, point);
    native::after_popup(focus);
    shown.map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Menu contents
// ---------------------------------------------------------------------------

/// The menu for `key`, or `None` for items that have none (separators).
fn item_menu(
    app: &AppHandle,
    state: &DockState,
    settings: &Settings,
    key: &str,
) -> Result<Option<Menu<Wry>>, String> {
    let item = state
        .items
        .iter()
        .find(|i| i.key == key)
        .ok_or_else(|| format!("unknown dock item: {key}"))?;
    let menu = MenuBuilder::new(app);
    let menu = match item.kind {
        DockItemKind::Separator => return Ok(None),
        DockItemKind::App => app_entries(app, menu, item, settings)?,
        DockItemKind::Launcher => shell_entry(menu, ShellAction::OpenStartMenu, "Open Start menu"),
        DockItemKind::TaskView => shell_entry(menu, ShellAction::OpenTaskView, "Open Task View"),
        DockItemKind::ShowDesktop => {
            shell_entry(menu, ShellAction::ToggleShowDesktop, "Show desktop")
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
/// *Close all windows*.
fn app_entries<'a>(
    app: &AppHandle,
    mut menu: Builder<'a>,
    item: &DockItem,
    settings: &Settings,
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
    if !item.windows.is_empty() {
        menu = menu.text(
            MenuAction::CloseAll(app_id.clone()).id(),
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
    CloseAll(AppId),
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
            Self::CloseAll(app) => format!("{ID_PREFIX}close-all:{}", app.as_str()),
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
            "close-all" => app().map(Self::CloseAll),
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

fn perform(service: &DockService, action: &MenuAction) -> ServiceResult<()> {
    match action {
        MenuAction::Activate(id) => service.window_action(*id, WindowAction::Activate),
        MenuAction::Launch(app) => service.launch_app(app),
        MenuAction::Pin(app) => service.pin_app(app),
        MenuAction::Unpin(app) => service.unpin_app(app),
        MenuAction::CloseAll(app) => close_all(service, app),
        MenuAction::Shell(shell) => service.shell_action(*shell),
        // Handled on the main thread by `on_menu_event`.
        MenuAction::Settings | MenuAction::Quit => Ok(()),
    }
}

/// Close every window the app has *now* (the list may have changed since
/// the menu was built). Every window is tried; the first error is returned.
fn close_all(service: &DockService, app_id: &AppId) -> ServiceResult<()> {
    let state = service.state();
    let windows: Vec<WindowId> = state
        .items
        .iter()
        .filter(|item| item.app_id.as_ref() == Some(app_id))
        .flat_map(|item| item.windows.iter().map(|w| w.id))
        .collect();
    if windows.is_empty() {
        log::debug!("close all: {app_id:?} has no windows any more");
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

// ---------------------------------------------------------------------------
// Win32 popup helper (the only Win32 outside stem-platform; see module docs)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod native {
    use tauri::WebviewWindow;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, IsWindow, PostMessageW, SetForegroundWindow, WM_NULL,
    };

    /// The windows involved in one popup: ours, and the one that had the
    /// foreground before muda gave it to ours.
    #[derive(Debug)]
    pub struct Foreground {
        dock: HWND,
        previous: HWND,
    }

    pub fn before_popup(window: &WebviewWindow) -> Option<Foreground> {
        let dock = match window.hwnd() {
            Ok(hwnd) => HWND(hwnd.0),
            Err(e) => {
                log::warn!("no HWND for {:?}: {e}", window.label());
                return None;
            }
        };
        // SAFETY: GetForegroundWindow has no preconditions; it returns a null
        // handle when no window is in the foreground.
        let previous = unsafe { GetForegroundWindow() };
        Some(Foreground { dock, previous })
    }

    /// After `TrackPopupMenu` returned: the `WM_NULL` of the documented
    /// recipe (KB 135788) so the next click outside a menu is delivered as
    /// such, then hand the foreground back to whoever had it. The dock is
    /// `WS_EX_NOACTIVATE`; it should never stay focused.
    pub fn after_popup(focus: Option<Foreground>) {
        let Some(Foreground { dock, previous }) = focus else {
            return;
        };
        // SAFETY: posts a no-op message to a window of this process; the
        // handle came from Tauri's live window. A dead handle only fails.
        if let Err(e) = unsafe { PostMessageW(Some(dock), WM_NULL, WPARAM(0), LPARAM(0)) } {
            log::debug!("PostMessage(WM_NULL) after the context menu failed: {e}");
        }
        if previous.0.is_null() || previous == dock {
            return;
        }
        // SAFETY: IsWindow accepts any handle value.
        if !unsafe { IsWindow(Some(previous)) }.as_bool() {
            return;
        }
        // SAFETY: SetForegroundWindow has no memory preconditions; it merely
        // fails when this process no longer holds the foreground right.
        if !unsafe { SetForegroundWindow(previous) }.as_bool() {
            log::debug!(
                "could not hand the foreground back to {:#x} after the context menu",
                previous.0 as isize
            );
        }
    }
}

#[cfg(not(windows))]
mod native {
    use tauri::WebviewWindow;

    #[derive(Debug)]
    pub struct Foreground;

    pub fn before_popup(_window: &WebviewWindow) -> Option<Foreground> {
        None
    }

    pub fn after_popup(_focus: Option<Foreground>) {}
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
            MenuAction::CloseAll(AppId("class:notepad".into())),
            MenuAction::Shell(ShellAction::EmptyRecycleBin),
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
        assert_eq!(MenuAction::Activate(WindowId(7)).id(), "ctx:activate:7");
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
            "ctx:shell:OpenStartMenu",
            "ctx:shell:",
            "ctx:close-all",
        ] {
            assert_eq!(MenuAction::parse(id), None, "{id}");
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
