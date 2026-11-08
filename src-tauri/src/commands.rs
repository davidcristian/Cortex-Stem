//! Tauri commands, one per entry in `docs/ipc/commands.md` (v1.11). They
//! validate and clamp their arguments, then translate and delegate to
//! `stem-service` (semantics live there).
//!
//! Errors cross IPC as plain strings (`Result<T, String>`); every failure is
//! also logged here at `warn` with the command name and its arguments.
//!
//! Threading: Tauri runs a synchronous command on the main thread and an
//! `async fn` command on its async runtime's thread pool. Every command that
//! reaches the platform (Win32 calls with bounded but real latency: icon
//! extraction, `SetForegroundWindow`, `ShellExecuteExW`, shortcut
//! resolution) or the disk (settings persistence) is therefore `async`, so
//! the main thread, which also pumps the dock window's messages, never
//! blocks on them. The bodies contain no `.await`: they simply run off the
//! main thread. The `get_*` getters (a mutex read) and the window-geometry
//! commands (which must touch the window on the main thread) stay
//! synchronous. `delete_to_recycle_bin` can wait on the user for as long as
//! the shell's dialogs are up, so it goes one step further and hands the
//! call to Tauri's blocking pool, keeping the async runtime's workers free.

use std::fmt::{Debug, Display};
use std::sync::Arc;

use base64::Engine as _;
use stem_core::{
    AppId, DockState, IconKey, LaunchTarget, MonitorInfo, Rect, Settings, ShellAction, ThemeInfo,
    WindowAction, WindowId,
};
use tauri::{AppHandle, State};

use crate::AppService;

type CmdResult<T> = Result<T, String>;

/// Icon sizes the UI may request (px); anything else is clamped.
const ICON_SIZE_RANGE: (u32, u32) = (16, 512);

/// Log a failed command with its arguments and turn the error into the IPC
/// string.
fn checked<T, E: Display>(command: &str, args: &dyn Debug, result: Result<T, E>) -> CmdResult<T> {
    result.map_err(|e| {
        log::warn!("{command}({args:?}) failed: {e}");
        e.to_string()
    })
}

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        get_dock_state,
        get_settings,
        update_settings,
        get_theme,
        get_monitors,
        primary_click,
        window_action,
        launch_app,
        pin_app,
        pin_target,
        unpin_app,
        reorder_pinned,
        get_icon,
        set_dock_content_size,
        set_dock_hit_region,
        set_dock_panel_rect,
        set_dock_auto_hidden,
        shell_action,
        delete_to_recycle_bin,
        open_settings,
        reset_settings,
        quit,
        crate::menus::show_item_menu,
        crate::menus::show_dock_menu,
    ]
}

#[tauri::command]
fn get_dock_state(service: State<'_, AppService>) -> DockState {
    service.0.state()
}

#[tauri::command]
fn get_settings(service: State<'_, AppService>) -> Settings {
    service.0.settings()
}

/// The service migrates, validates (clamps) and persists; the validated
/// copy comes back.
#[tauri::command]
async fn update_settings(
    service: State<'_, AppService>,
    settings: Settings,
) -> CmdResult<Settings> {
    checked(
        "update_settings",
        &settings,
        service.0.update_settings(settings.clone()),
    )
}

#[tauri::command]
fn get_theme(service: State<'_, AppService>) -> ThemeInfo {
    service.0.theme()
}

#[tauri::command]
fn get_monitors(service: State<'_, AppService>) -> Vec<MonitorInfo> {
    service.0.monitors()
}

#[tauri::command]
async fn primary_click(service: State<'_, AppService>, item_key: String) -> CmdResult<()> {
    checked(
        "primary_click",
        &item_key,
        service.0.primary_click(&item_key),
    )
}

#[tauri::command]
async fn window_action(
    service: State<'_, AppService>,
    window_id: WindowId,
    action: WindowAction,
) -> CmdResult<()> {
    checked(
        "window_action",
        &(window_id, action),
        service.0.window_action(window_id, action),
    )
}

#[tauri::command]
async fn launch_app(service: State<'_, AppService>, app_id: AppId) -> CmdResult<()> {
    checked("launch_app", &app_id, service.0.launch_app(&app_id))
}

#[tauri::command]
async fn pin_app(service: State<'_, AppService>, app_id: AppId) -> CmdResult<()> {
    checked("pin_app", &app_id, service.0.pin_app(&app_id))
}

#[tauri::command]
async fn pin_target(service: State<'_, AppService>, target: LaunchTarget) -> CmdResult<AppId> {
    checked("pin_target", &target, service.0.pin_target(target.clone()))
}

#[tauri::command]
async fn unpin_app(service: State<'_, AppService>, app_id: AppId) -> CmdResult<()> {
    checked("unpin_app", &app_id, service.0.unpin_app(&app_id))
}

#[tauri::command]
async fn reorder_pinned(service: State<'_, AppService>, order: Vec<AppId>) -> CmdResult<()> {
    checked("reorder_pinned", &order, service.0.reorder_pinned(&order))
}

/// PNG icon as a `data:` URL, or `None` for unknown keys and `shell:*` keys.
#[tauri::command]
async fn get_icon(
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

/// The dock UI reports the size of its visible content (CSS pixels) so the
/// window can be resized and re-centred. See `docs/ipc/sizing-protocol.md`. Only a
/// dock window may call it, with finite numbers; the range is clamped in
/// `windows::apply_content_size`. Synchronous on purpose: it touches the
/// window and must run on the main thread.
#[tauri::command]
fn set_dock_content_size(
    app: AppHandle,
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> CmdResult<()> {
    let args = (window.label().to_owned(), width, height);
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "set_dock_content_size",
            &args,
            Err::<(), _>("only dock windows can be resized"),
        );
    }
    if !(width.is_finite() && height.is_finite()) {
        return checked(
            "set_dock_content_size",
            &args,
            Err::<(), _>("width and height must be finite numbers"),
        );
    }
    checked(
        "set_dock_content_size",
        &args,
        crate::windows::apply_content_size(&app, &window, width, height),
    )
}

/// The dock UI reports where it accepts clicks (CSS px, window-local; see
/// `docs/ipc/hit-region.md`). Only a dock window may call it; the rects
/// must have non-negative sizes and `envelope` must contain `panel`.
/// Synchronous: it reads the window's scale factor on the main thread and
/// the platform call only hands the region to its tracking thread.
#[tauri::command]
fn set_dock_hit_region(
    app: AppHandle,
    window: tauri::WebviewWindow,
    panel: Rect,
    envelope: Rect,
) -> CmdResult<()> {
    let args = (window.label().to_owned(), panel, envelope);
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "set_dock_hit_region",
            &args,
            Err::<(), _>("only dock windows have a hit region"),
        );
    }
    if let Err(e) = crate::windows::validate_hit_region(panel, envelope) {
        return checked("set_dock_hit_region", &args, Err::<(), _>(e));
    }
    checked(
        "set_dock_hit_region",
        &args,
        crate::windows::apply_hit_region(&app, &window, panel, envelope),
    )
}

/// The dock UI reports where its panel rests (CSS px relative to the
/// viewport, whatever the auto-hide phase; see `docs/ipc/dock-panel.md`),
/// so the engine can tell when a window overlaps it. Only a dock
/// window may call it; the rect must have non-negative sizes. Synchronous:
/// it reads the window's position and scale factor on the main thread, and
/// the service call only queues the converted rect for its worker.
#[tauri::command]
fn set_dock_panel_rect(app: AppHandle, window: tauri::WebviewWindow, rect: Rect) -> CmdResult<()> {
    let args = (window.label().to_owned(), rect);
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "set_dock_panel_rect",
            &args,
            Err::<(), _>("only dock windows have a panel"),
        );
    }
    if let Err(e) = crate::windows::validate_panel_rect(rect) {
        return checked("set_dock_panel_rect", &args, Err::<(), _>(e));
    }
    checked(
        "set_dock_panel_rect",
        &args,
        crate::windows::apply_dock_panel(&app, &window, rect),
    )
}

/// The dock UI reports that auto-hide slid its panel out, or brought it
/// back (`docs/ipc/toggle-while-hidden.md`), so the tray click and
/// the toggle shortcut can bring the panel back instead of hiding the
/// window. Only a dock window may call it. Synchronous: a flag in managed
/// state, nothing else.
#[tauri::command]
fn set_dock_auto_hidden(
    app: AppHandle,
    window: tauri::WebviewWindow,
    hidden: bool,
) -> CmdResult<()> {
    let args = (window.label().to_owned(), hidden);
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "set_dock_auto_hidden",
            &args,
            Err::<(), _>("only dock windows auto-hide"),
        );
    }
    checked(
        "set_dock_auto_hidden",
        &args,
        crate::windows::set_auto_hidden(&app, &window, hidden),
    )
}

/// Run a shell action directly (menus, Recycle Bin "Empty").
#[tauri::command]
async fn shell_action(service: State<'_, AppService>, action: ShellAction) -> CmdResult<()> {
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
async fn delete_to_recycle_bin(
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

/// Open the settings window, or focus it if it is already open.
#[tauri::command]
fn open_settings(app: AppHandle) -> CmdResult<()> {
    checked(
        "open_settings",
        &(),
        crate::windows::open_settings_window(&app),
    )
}

/// Restore and persist the defaults; the service emits `dock://settings`.
#[tauri::command]
async fn reset_settings(service: State<'_, AppService>) -> CmdResult<Settings> {
    checked(
        "reset_settings",
        &(),
        service.0.update_settings(Settings::default()),
    )
}

#[tauri::command]
fn quit(app: AppHandle) {
    log::info!("quit requested from the UI");
    crate::request_quit(&app);
}
