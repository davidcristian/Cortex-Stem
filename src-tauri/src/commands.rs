//! Tauri commands, one per entry in `docs/ipc/commands.md` (v1.1). They
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
//! synchronous.

use std::fmt::{Debug, Display};

use base64::Engine as _;
use stem_core::{
    AppId, DockState, IconKey, LaunchTarget, MonitorInfo, Settings, ShellAction, ThemeInfo,
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
        shell_action,
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

/// Run a shell action directly (menus, Recycle Bin "Empty").
#[tauri::command]
async fn shell_action(service: State<'_, AppService>, action: ShellAction) -> CmdResult<()> {
    checked("shell_action", &action, service.0.shell_action(action))
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
