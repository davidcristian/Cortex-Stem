//! The commands for the app itself: the settings window, quitting, and
//! the settings page's About section.

use std::sync::Arc;

use stem_core::{AppFolder, AppInfo, LaunchTarget};
use stem_platform::is_mock;
use tauri::{AppHandle, State};

use super::{CmdResult, checked};
use crate::{AppPaths, AppService};

/// Open the settings window, or focus it if it is already open.
#[tauri::command]
pub(super) fn open_settings(app: AppHandle) -> CmdResult<()> {
    checked(
        "open_settings",
        &(),
        crate::windows::open_settings_window(&app),
    )
}

#[tauri::command]
pub(super) fn quit(app: AppHandle) {
    log::info!("quit requested from the UI");
    crate::request_quit(&app);
}

/// What `AppInfo::settings_path` says when nothing is saved this run.
const SETTINGS_NOT_SAVED_DEMO: &str = "(demo: not saved)";
const SETTINGS_NOT_SAVED: &str = "(not saved this session)";

/// Version, settings file, log file and platform name for the settings
/// page's About section (`docs/ipc/about.md`). Synchronous: managed
/// state and a constant, nothing that blocks.
#[tauri::command]
pub(super) fn get_app_info(service: State<'_, AppService>, paths: State<'_, AppPaths>) -> AppInfo {
    let platform = service.0.platform();
    let settings_path = match &paths.settings_file {
        Some(file) => file.display().to_string(),
        None if is_mock(platform.as_ref()) => SETTINGS_NOT_SAVED_DEMO.to_owned(),
        None => SETTINGS_NOT_SAVED.to_owned(),
    };
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        settings_path,
        log_path: paths
            .log_file
            .as_ref()
            .map(|file| file.display().to_string())
            .unwrap_or_default(),
        platform: platform.name().to_owned(),
    }
}

/// Open the log folder or the settings file's folder in Explorer
/// (`docs/ipc/about.md`): `AppPaths::folder_to_open` picks the folder and
/// `Platform::launch` hands its path to the shell (`ShellExecuteExW`, which
/// opens a folder in Explorer). Both touch the disk or the shell, so they
/// run on Tauri's blocking pool like `delete_to_recycle_bin`; the command
/// resolves once the shell has taken the request. A failure (no settings
/// file in this run, a folder that does not exist, the shell's refusal) is
/// the command's error string.
#[tauri::command]
pub(super) async fn open_app_folder(
    service: State<'_, AppService>,
    paths: State<'_, AppPaths>,
    folder: AppFolder,
) -> CmdResult<()> {
    let service = Arc::clone(&service.0);
    let paths = paths.inner().clone();
    let joined = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let dir = paths.folder_to_open(folder)?;
        let target = LaunchTarget::Uri {
            uri: dir.to_string_lossy().into_owned(),
        };
        service
            .platform()
            .launch(&target)
            .map_err(|e| e.to_string())?;
        log::info!("open_app_folder({folder:?}): opened {}", dir.display());
        Ok(())
    })
    .await;
    match joined {
        Ok(result) => checked("open_app_folder", &folder, result),
        Err(e) => checked(
            "open_app_folder",
            &folder,
            Err::<(), _>(format!("the open task did not finish: {e}")),
        ),
    }
}
