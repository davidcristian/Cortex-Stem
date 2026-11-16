//! The commands for the user's `dock.css` (`docs/ipc/themes.md`).

use std::sync::Arc;

use stem_core::UserCss;
use tauri::{AppHandle, Manager, State};

use super::{CmdResult, checked};
use crate::AppService;
use crate::user_css::UserCssState;

/// The user's `dock.css` as read at start or by the last `reload_user_css`
/// (`docs/ipc/themes.md`). Synchronous: a mutex read.
#[tauri::command]
pub(super) fn get_user_css(user_css: State<'_, UserCssState>) -> UserCss {
    user_css.current()
}

/// Read `dock.css` again (`user_css::UserCssState::reload_and`, which logs
/// what came of it) and send the result to every dock window as
/// `dock://user-css`. The read touches the disk, so it runs on
/// Tauri's blocking pool like `open_app_folder`; the read, the stored
/// result and the emit happen under one lock, so two Reloads at once leave
/// the docks on the later read. The command resolves with what the docks
/// were sent.
#[tauri::command]
pub(super) async fn reload_user_css(app: AppHandle) -> CmdResult<UserCss> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        app.state::<UserCssState>()
            .reload_and(|css| crate::user_css::emit_to_docks(&app, css))
    })
    .await;
    checked(
        "reload_user_css",
        &(),
        joined.map_err(|e| format!("the read task did not finish: {e}")),
    )
}

/// Create `dock.css` from the template if it does not exist, then open it
/// with the default app for `.css`, or with Notepad when that fails
/// (`user_css::open_file`): `Platform::launch` hands the path to the
/// shell (`ShellExecuteExW`), like `open_app_folder` does a folder, on
/// Tauri's blocking pool. Nothing is read: the dock changes at the next
/// `reload_user_css`. A failure (no settings file in this run, a template
/// that cannot be written, neither the default app nor Notepad opening it)
/// is the command's error string.
#[tauri::command]
pub(super) async fn open_user_css(app: AppHandle, service: State<'_, AppService>) -> CmdResult<()> {
    let service = Arc::clone(&service.0);
    let joined = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let file = app.state::<UserCssState>().file_to_open()?;
        let notepad =
            crate::user_css::notepad_for(&file, std::env::var_os("SystemRoot").as_deref());
        let opener = crate::user_css::open_file(&file, &notepad, |target| {
            service.platform().launch(target).map_err(|e| e.to_string())
        })?;
        log::info!("open_user_css: opened {} {opener}", file.display());
        Ok(())
    })
    .await;
    match joined {
        Ok(result) => checked("open_user_css", &(), result),
        Err(e) => checked(
            "open_user_css",
            &(),
            Err::<(), _>(format!("the open task did not finish: {e}")),
        ),
    }
}
