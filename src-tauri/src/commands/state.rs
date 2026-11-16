//! The getters of the service's snapshot, settings, theme and monitors,
//! and the commands that write the settings.

use stem_core::{DockState, MonitorInfo, Settings, ThemeInfo};
use tauri::State;

use super::{CmdResult, checked};
use crate::AppService;

#[tauri::command]
pub(super) fn get_dock_state(service: State<'_, AppService>) -> DockState {
    service.0.state()
}

#[tauri::command]
pub(super) fn get_settings(service: State<'_, AppService>) -> Settings {
    service.0.settings()
}

/// The service migrates, validates (clamps) and persists, keeping its own
/// pinned and hidden apps (IPC v1.26); the validated copy comes back.
#[tauri::command]
pub(super) async fn update_settings(
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
pub(super) fn get_theme(service: State<'_, AppService>) -> ThemeInfo {
    service.0.theme()
}

#[tauri::command]
pub(super) fn get_monitors(service: State<'_, AppService>) -> Vec<MonitorInfo> {
    service.0.monitors()
}

/// Restore and persist the defaults, pins and hidden apps included (which
/// `update_settings` keeps); the service emits `dock://settings`.
#[tauri::command]
pub(super) async fn reset_settings(service: State<'_, AppService>) -> CmdResult<Settings> {
    checked("reset_settings", &(), service.0.reset_settings())
}
