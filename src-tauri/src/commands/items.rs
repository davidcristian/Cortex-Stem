//! The commands that act on the dock's items: clicks, wheel notches,
//! window actions, launches, pins, hidden apps, icons, shell actions and
//! drops on the Recycle Bin.

use std::sync::Arc;

use base64::Engine as _;
use stem_core::{
    AppId, IconKey, LaunchTarget, ScrollDirection, ShellAction, WindowAction, WindowId,
};
use tauri::{AppHandle, State};

use super::{CmdResult, checked};
use crate::AppService;

/// Icon sizes the UI may request (px); anything else is clamped.
const ICON_SIZE_RANGE: (u32, u32) = (16, 512);

/// A click on a dock item. The calling window's label names the dock it
/// happened in, whose own view of the windows the service acts on
/// (`docs/ipc/windows.md` §Dock windows per monitor); the payload is the
/// item key alone. Whatever it does supersedes the window the focus
/// shortcut would go back to. A click that would minimise the active app
/// while Stem's settings window is in front of it brings the app forward
/// instead (`stem_service::over_own_window`).
#[tauri::command]
pub(super) async fn primary_click(
    app: AppHandle,
    window: tauri::WebviewWindow,
    service: State<'_, AppService>,
    item_key: String,
) -> CmdResult<()> {
    crate::windows::forget_dock_focus(&app, "a click on an item");
    let dock = window.label();
    let own_window_in_front =
        || crate::windows::non_dock_window_in_front(&app, service.0.platform().as_ref());
    checked(
        "primary_click",
        &(dock, &item_key),
        service
            .0
            .primary_click_over(&item_key, dock, own_window_in_front),
    )
}

/// One wheel notch over a dock item (`docs/ipc/scrolling-over-an-item.md`).
/// As for `primary_click`, the calling window's label names the
/// dock whose own view of the windows the service cycles through, and the
/// service's 150 ms gate is that dock window's. A notch that activated a
/// window supersedes the window the focus shortcut would go back to,
/// as a click does; one that did nothing (the setting is `none`,
/// no windows, dropped by the gate) leaves it, and so does one that
/// stepped the volume over the volume item (IPC v1.30), which
/// brings no window forward. The service call runs on Tauri's blocking
/// pool: a volume step waits (bounded) on the platform's volume thread, and
/// a fast spin sends up to ten notches at once, which must not hold the
/// async runtime's workers that every other command runs on.
#[tauri::command]
pub(super) async fn scroll_item(
    app: AppHandle,
    window: tauri::WebviewWindow,
    service: State<'_, AppService>,
    item_key: String,
    direction: ScrollDirection,
) -> CmdResult<()> {
    let dock = window.label().to_owned();
    let args = (dock.clone(), item_key.clone(), direction);
    let service = Arc::clone(&service.0);
    let joined = tauri::async_runtime::spawn_blocking(move || {
        service.scroll_item(&item_key, &dock, direction)
    })
    .await;
    let activated = match joined {
        Ok(result) => checked("scroll_item", &args, result)?,
        Err(e) => checked(
            "scroll_item",
            &args,
            Err::<Option<stem_core::WindowId>, _>(format!("the scroll task did not finish: {e}")),
        )?,
    };
    if activated.is_some() {
        crate::windows::forget_dock_focus(&app, "a scroll over an item");
    }
    Ok(())
}

/// A direct action on one window; like a click, it supersedes the window
/// the focus shortcut would go back to.
#[tauri::command]
pub(super) async fn window_action(
    app: AppHandle,
    service: State<'_, AppService>,
    window_id: WindowId,
    action: WindowAction,
) -> CmdResult<()> {
    crate::windows::forget_dock_focus(&app, "a window action");
    checked(
        "window_action",
        &(window_id, action),
        service.0.window_action(window_id, action),
    )
}

/// Start a new instance; like a click, it supersedes the window the focus
/// shortcut would go back to.
#[tauri::command]
pub(super) async fn launch_app(
    app: AppHandle,
    service: State<'_, AppService>,
    app_id: AppId,
) -> CmdResult<()> {
    crate::windows::forget_dock_focus(&app, "a launch");
    checked("launch_app", &app_id, service.0.launch_app(&app_id))
}

#[tauri::command]
pub(super) async fn pin_app(service: State<'_, AppService>, app_id: AppId) -> CmdResult<()> {
    checked("pin_app", &app_id, service.0.pin_app(&app_id))
}

#[tauri::command]
pub(super) async fn pin_target(
    service: State<'_, AppService>,
    target: LaunchTarget,
) -> CmdResult<AppId> {
    checked("pin_target", &target, service.0.pin_target(target.clone()))
}

#[tauri::command]
pub(super) async fn unpin_app(service: State<'_, AppService>, app_id: AppId) -> CmdResult<()> {
    checked("unpin_app", &app_id, service.0.unpin_app(&app_id))
}

/// Stop showing a running, unpinned app on every dock
/// (`docs/ipc/hidden-apps.md`); the service saves it and rejects a pinned
/// or unknown app.
#[tauri::command]
pub(super) async fn hide_app(service: State<'_, AppService>, app_id: AppId) -> CmdResult<()> {
    checked("hide_app", &app_id, service.0.hide_app(&app_id))
}

/// Show a hidden app again; an id that is not hidden is no error.
#[tauri::command]
pub(super) async fn unhide_app(service: State<'_, AppService>, app_id: AppId) -> CmdResult<()> {
    checked("unhide_app", &app_id, service.0.unhide_app(&app_id))
}

#[tauri::command]
pub(super) async fn reorder_pinned(
    service: State<'_, AppService>,
    order: Vec<AppId>,
) -> CmdResult<()> {
    checked("reorder_pinned", &order, service.0.reorder_pinned(&order))
}

/// PNG icon as a `data:` URL, or `None` for unknown keys and `shell:*` keys.
#[tauri::command]
pub(super) async fn get_icon(
    service: State<'_, AppService>,
    icon_key: IconKey,
    size: u32,
) -> CmdResult<Option<String>> {
    let size = size.clamp(ICON_SIZE_RANGE.0, ICON_SIZE_RANGE.1);
    let png = checked(
        "get_icon",
        &(&icon_key, size),
        service.0.icon_png(&icon_key, size),
    )?;
    Ok(png.map(|bytes| {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }))
}

/// Run a shell action directly (menus, Recycle Bin "Empty").
#[tauri::command]
pub(super) async fn shell_action(
    service: State<'_, AppService>,
    action: ShellAction,
) -> CmdResult<()> {
    checked("shell_action", &action, service.0.shell_action(action))
}

/// Move the files and folders of a drop on the Recycle Bin item to the bin
/// (`docs/ipc/file-drags.md`). Only a dock window may call it. The
/// shell's file operation waits for the user while its dialogs are up, so
/// the service call runs on Tauri's blocking pool rather than on a worker
/// of the async runtime (the shell work itself runs on a thread of the
/// platform's own); the command resolves when the shell is done. A failure
/// is logged with the window, the number of paths and the first one.
#[tauri::command]
pub(super) async fn delete_to_recycle_bin(
    service: State<'_, AppService>,
    window: tauri::WebviewWindow,
    paths: Vec<String>,
) -> CmdResult<()> {
    let args = (
        window.label().to_owned(),
        paths.len(),
        paths.first().cloned(),
    );
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "delete_to_recycle_bin",
            &args,
            Err::<(), _>("only dock windows take file drops"),
        );
    }
    let service = Arc::clone(&service.0);
    let joined =
        tauri::async_runtime::spawn_blocking(move || service.delete_to_recycle_bin(&paths)).await;
    match joined {
        Ok(result) => checked("delete_to_recycle_bin", &args, result),
        Err(e) => checked(
            "delete_to_recycle_bin",
            &args,
            Err::<(), _>(format!("the delete task did not finish: {e}")),
        ),
    }
}
