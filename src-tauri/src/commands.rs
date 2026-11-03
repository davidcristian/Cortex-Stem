//! Tauri commands, one per entry in `docs/ipc/commands.md`. They validate
//! nothing themselves; they translate and delegate to `stem-service`.
//!
//! Errors cross IPC as plain strings (`Result<T, String>`); the frontend shows
//! or logs them. Commands must return quickly: anything slow belongs on the
//! service worker thread or the platform's own threads.

use base64::Engine as _;
use stem_core::{
    AppId, DockState, IconKey, LaunchTarget, MonitorInfo, Settings, ThemeInfo, WindowAction,
    WindowId,
};
use tauri::{AppHandle, State};

use crate::AppService;

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
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
        quit,
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

#[tauri::command]
fn update_settings(service: State<'_, AppService>, settings: Settings) -> CmdResult<Settings> {
    service.0.update_settings(settings).map_err(err)
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
fn primary_click(service: State<'_, AppService>, item_key: String) -> CmdResult<()> {
    service.0.primary_click(&item_key).map_err(err)
}

#[tauri::command]
fn window_action(
    service: State<'_, AppService>,
    window_id: WindowId,
    action: WindowAction,
) -> CmdResult<()> {
    service.0.window_action(window_id, action).map_err(err)
}

#[tauri::command]
fn launch_app(service: State<'_, AppService>, app_id: AppId) -> CmdResult<()> {
    service.0.launch_app(&app_id).map_err(err)
}

#[tauri::command]
fn pin_app(service: State<'_, AppService>, app_id: AppId) -> CmdResult<()> {
    service.0.pin_app(&app_id).map_err(err)
}

#[tauri::command]
fn pin_target(service: State<'_, AppService>, target: LaunchTarget) -> CmdResult<AppId> {
    service.0.pin_target(target).map_err(err)
}

#[tauri::command]
fn unpin_app(service: State<'_, AppService>, app_id: AppId) -> CmdResult<()> {
    service.0.unpin_app(&app_id).map_err(err)
}

#[tauri::command]
fn reorder_pinned(service: State<'_, AppService>, order: Vec<AppId>) -> CmdResult<()> {
    service.0.reorder_pinned(&order).map_err(err)
}

/// PNG icon as a `data:` URL, or `None` for unknown keys and `shell:*` keys.
#[tauri::command]
fn get_icon(
    service: State<'_, AppService>,
    icon_key: IconKey,
    size: u32,
) -> CmdResult<Option<String>> {
    let png = service
        .0
        .icon_png(&icon_key, size.clamp(16, 512))
        .map_err(err)?;
    Ok(png.map(|bytes| {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }))
}

/// The dock UI reports the size of its visible content (CSS pixels) so the
/// window can be resized and re-centred. See `docs/ipc/`.
#[tauri::command]
fn set_dock_content_size(
    app: AppHandle,
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> CmdResult<()> {
    crate::windows::apply_content_size(&app, &window, width, height).map_err(err)
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}
