//! Tauri commands, one per entry in `docs/ipc/commands.md` (v1.26). They
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
//! template, then the shell), and `scroll_item` (a notch over the
//! volume item waits on the platform's volume thread).

mod app;
mod dock_window;
mod items;
mod state;
mod user_css;

use std::fmt::{Debug, Display};

type CmdResult<T> = Result<T, String>;

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
        state::get_dock_state,
        state::get_settings,
        state::update_settings,
        state::get_theme,
        state::get_monitors,
        items::primary_click,
        items::scroll_item,
        items::window_action,
        items::launch_app,
        items::pin_app,
        items::pin_target,
        items::unpin_app,
        items::hide_app,
        items::unhide_app,
        items::reorder_pinned,
        items::get_icon,
        dock_window::set_dock_content_size,
        dock_window::set_dock_hit_region,
        dock_window::set_dock_panel_rect,
        dock_window::set_dock_auto_hidden,
        items::shell_action,
        items::delete_to_recycle_bin,
        app::open_settings,
        state::reset_settings,
        app::quit,
        app::get_app_info,
        app::open_app_folder,
        crate::menus::show_item_menu,
        crate::menus::show_dock_menu,
        dock_window::release_dock_focus,
        user_css::get_user_css,
        user_css::reload_user_css,
        user_css::open_user_css,
    ]
}
