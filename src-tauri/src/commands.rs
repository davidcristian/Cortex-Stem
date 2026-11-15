//! Tauri commands, one per entry in `docs/ipc/commands.md` (v1.15). They
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
//! call to Tauri's blocking pool, keeping the async runtime's workers free;
//! `open_app_folder` (disk checks, then the shell) does the same, and so do
//! `reload_user_css` (a read of `dock.css`) and `open_user_css` (the
//! template, then the shell).

use std::fmt::{Debug, Display};
use std::sync::Arc;

use base64::Engine as _;
use stem_core::settings::DockPosition;
use stem_core::{
    AppFolder, AppId, AppInfo, DockState, IconKey, LaunchTarget, MonitorInfo, Rect, Settings,
    ShellAction, ThemeInfo, UserCss, WindowAction, WindowId,
};
use stem_platform::is_mock;
use tauri::{AppHandle, Manager, State};

use crate::user_css::UserCssState;
use crate::windows::{CssViewport, PanelReport};
use crate::{AppPaths, AppService};

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
        get_app_info,
        open_app_folder,
        crate::menus::show_item_menu,
        crate::menus::show_dock_menu,
        release_dock_focus,
        get_user_css,
        reload_user_css,
        open_user_css,
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

/// A click on a dock item. The calling window's label names the dock it
/// happened in, whose own view of the windows the service acts on
/// (`docs/ipc/windows.md` §Dock windows per monitor); the payload is the item key
/// alone. Whatever it does supersedes the window the focus shortcut would
/// go back to.
#[tauri::command]
async fn primary_click(
    app: AppHandle,
    window: tauri::WebviewWindow,
    service: State<'_, AppService>,
    item_key: String,
) -> CmdResult<()> {
    crate::windows::forget_dock_focus(&app, "a click on an item");
    let dock = window.label();
    checked(
        "primary_click",
        &(dock, &item_key),
        service.0.primary_click(&item_key, dock),
    )
}

/// A direct action on one window; like a click, it supersedes the window
/// the focus shortcut would go back to.
#[tauri::command]
async fn window_action(
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
async fn launch_app(
    app: AppHandle,
    service: State<'_, AppService>,
    app_id: AppId,
) -> CmdResult<()> {
    crate::windows::forget_dock_focus(&app, "a launch");
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
/// window can take its thickness from it: the height on a horizontal dock
/// (`bottom`, `top`), the width on a vertical one (`left`, `right`); the
/// window stays as long as its work area along the edge. See
/// `docs/ipc/sizing-protocol.md`. `edge` is the edge the page laid itself
/// out for (docs/ipc/report-provenance.md, v1.18): a report for another
/// edge than the window's is dropped there. Only a dock window may call
/// it, with finite numbers; the range is clamped in
/// `windows::apply_content_size`. Synchronous on purpose: it touches the
/// window and must run on the main thread.
#[tauri::command]
fn set_dock_content_size(
    app: AppHandle,
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
    edge: DockPosition,
) -> CmdResult<()> {
    let args = (window.label().to_owned(), width, height, edge);
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
        crate::windows::apply_content_size(&app, &window, width, height, edge),
    )
}

/// The dock UI reports where it accepts clicks (CSS px, window-local; see
/// `docs/ipc/hit-region.md`), laid out for `edge`
/// (docs/ipc/report-provenance.md: a report for another edge than the
/// window's is dropped). Only a dock window may call it; the rects must
/// have non-negative sizes and `envelope` must contain `panel`.
/// Synchronous: it reads the window's scale factor on the main thread and
/// the platform call only hands the region to its tracking thread.
#[tauri::command]
fn set_dock_hit_region(
    app: AppHandle,
    window: tauri::WebviewWindow,
    panel: Rect,
    envelope: Rect,
    edge: DockPosition,
) -> CmdResult<()> {
    let args = (window.label().to_owned(), panel, envelope, edge);
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
        crate::windows::apply_hit_region(&app, &window, panel, envelope, edge),
    )
}

/// The dock UI reports where its panel rests (CSS px relative to
/// `viewport`, whatever the auto-hide phase; see
/// `docs/ipc/dock-panel.md`), so the engine can tell when a window
/// overlaps it; the app layer hands it on with the calling window's label
/// and monitor, so each dock has its own fact. `edge` and `viewport` say
/// what the page measured it in (docs/ipc/report-provenance.md, v1.18): a
/// report for another edge than the window's is dropped, and the rect is
/// re-anchored to the window's viewport of the moment before it is
/// converted. Only a dock window may call it; the rect must have
/// non-negative sizes and the viewport a positive, finite width and
/// height. Synchronous: it reads the window's position, size and scale
/// factor on the main thread, and the service call only queues the
/// converted rect for its worker.
#[tauri::command]
fn set_dock_panel_rect(
    app: AppHandle,
    window: tauri::WebviewWindow,
    rect: Rect,
    edge: DockPosition,
    viewport: CssViewport,
) -> CmdResult<()> {
    let args = (window.label().to_owned(), rect, edge, viewport);
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "set_dock_panel_rect",
            &args,
            Err::<(), _>("only dock windows have a panel"),
        );
    }
    if let Err(e) = crate::windows::validate_panel_rect(rect).and_then(|()| viewport.validate()) {
        return checked("set_dock_panel_rect", &args, Err::<(), _>(e));
    }
    let report = PanelReport {
        rect,
        edge,
        viewport,
    };
    checked(
        "set_dock_panel_rect",
        &args,
        crate::windows::apply_dock_panel(&app, &window, report),
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

/// The calling dock window gives the keyboard focus back (Escape, or a
/// second press of the focus shortcut through `dock://release-focus`;
/// `docs/ipc/keyboard-entry.md`): the window that was in front
/// when the focus shortcut was pressed comes back to the foreground, if the
/// focus shortcut's session is this dock's and that window still exists
/// (a click on a dock forgets it); otherwise the foreground is left alone
/// and the command resolves. Only a dock window may call it. `async`: the
/// activation is the foreground chain, which must not run on the main thread.
#[tauri::command]
async fn release_dock_focus(
    window: tauri::WebviewWindow,
    service: State<'_, AppService>,
    focus: State<'_, crate::windows::DockFocus>,
) -> CmdResult<()> {
    let label = window.label().to_owned();
    if !label.starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "release_dock_focus",
            &label,
            Err::<(), _>("only dock windows take the keyboard focus"),
        );
    }
    checked(
        "release_dock_focus",
        &label,
        crate::windows::give_focus_back(&focus, service.0.platform().as_ref(), &label),
    )
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

/// What `AppInfo::settings_path` says when nothing is saved this run.
const SETTINGS_NOT_SAVED_DEMO: &str = "(demo: not saved)";
const SETTINGS_NOT_SAVED: &str = "(not saved this session)";

/// Version, settings file, log file and platform name for the settings
/// page's About section (`docs/ipc/about.md`). Synchronous: managed
/// state and a constant, nothing that blocks.
#[tauri::command]
fn get_app_info(service: State<'_, AppService>, paths: State<'_, AppPaths>) -> AppInfo {
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
async fn open_app_folder(
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

/// The user's `dock.css` as read at start or by the last `reload_user_css`
/// (`docs/ipc/themes.md`). Synchronous: a mutex read.
#[tauri::command]
fn get_user_css(user_css: State<'_, UserCssState>) -> UserCss {
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
async fn reload_user_css(app: AppHandle) -> CmdResult<UserCss> {
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
async fn open_user_css(app: AppHandle, service: State<'_, AppService>) -> CmdResult<()> {
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
