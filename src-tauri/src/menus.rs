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
//! `ctx:shell:<shellAction>` (camelCase, as serialised), `ctx:mute:on` and
//! `ctx:mute:off` (the volume item's *Mute* and *Unmute*),
//! `ctx:settings`, `ctx:quit`. Everything starts with `ctx:` because the tray's items
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

mod actions;
mod build;
#[cfg(test)]
mod tests;

use stem_service::DockService;
use tauri::menu::{ContextMenu, Menu};
use tauri::{AppHandle, PhysicalPosition, State, WebviewWindow, Wry};

use actions::on_menu_event;
use build::{dock_menu, item_menu};

use crate::AppService;

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
        let volume = service.0.volume();
        let Some(menu) = item_menu(&app, &item, &settings, dock, volume)? else {
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
