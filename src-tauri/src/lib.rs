//! The Tauri shell. Thin by design: every decision lives in `stem-service`
//! and below. This crate only
//! - creates and positions the dock window(s) (one per monitor with
//!   `monitors = all`) and the settings window,
//! - exposes `stem-service` through Tauri commands (`commands`),
//! - forwards `ServiceEvent`s as Tauri events (`docs/ipc/events.md`),
//! - wires plugins (single-instance, logging, autostart, global shortcut)
//!   and the tray icon, and applies the settings that belong to the shell
//!   (`autostart`, `taskbar`, `appbar`, `shortcut`, click-through in
//!   `windows`, the tray's check mark, the native backdrop prototype in
//!   `backdrop`),
//! - reports a crash of the previous run at the next start (`crash`),
//! - knows where its own files are (`AppPaths`) for the settings page's
//!   About section, and reads the user's `dock.css` (`user_css`).
//!
//! Lifecycle: this is a tray application. The dock window ignores close
//! requests, closing the settings window only destroys that window, and an
//! exit requested "by user interaction" (`RunEvent::ExitRequested` with no
//! code) is refused unless [`request_quit`] was called (tray *Quit* or the
//! `quit` command).

mod app;
mod appbar;
mod autostart;
mod backdrop;
mod commands;
mod crash;
mod menus;
mod shortcut;
mod taskbar;
mod tray;
mod user_css;
mod windows;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use stem_platform::{create_platform, is_mock};
use stem_service::{DockService, ServiceOptions};
use tauri::{AppHandle, Manager, RunEvent};

use app::forward::forward_events;
use app::logging::init_logging;
use app::panic_hook::{install_panic_hook, panic_test};
pub use app::paths::AppPaths;
use app::service::{icon_cache_dir, settings_store, start_service};

/// Shared handle to the service, stored in Tauri's managed state.
#[derive(Debug, Clone)]
pub struct AppService(pub Arc<DockService>);

/// Event names, as documented in `docs/ipc/events.md`. All but `SHOW`,
/// `FOCUS`, `RELEASE_FOCUS`, `USER_CSS` and `SETTINGS_CLOSING` forward a
/// `ServiceEvent`; the first three come from the shell itself
/// (`windows::toggle_dock`, `windows::on_focus_shortcut`), `USER_CSS` from
/// `reload_user_css`, `SETTINGS_CLOSING` from the settings window's close
/// request (`windows::open_settings_window`).
pub mod events {
    pub const STATE: &str = "dock://state";
    pub const SETTINGS: &str = "dock://settings";
    pub const THEME: &str = "dock://theme";
    pub const MONITORS: &str = "dock://monitors";
    pub const BOUNCE: &str = "dock://item-bounce";
    /// To one dock window: bring the auto-hidden panel back.
    pub const SHOW: &str = "dock://show";
    /// To one dock window: the focus shortcut gave it the keyboard focus;
    /// focus the toolbar.
    pub const FOCUS: &str = "dock://focus";
    /// To one dock window: a press of the focus shortcut while it still
    /// has the keyboard from an earlier press; do what Escape does.
    pub const RELEASE_FOCUS: &str = "dock://release-focus";
    /// To every dock window: `dock.css` was read again.
    pub const USER_CSS: &str = "dock://user-css";
    /// To the settings window: it is closing; send any edit still held
    /// back now (`windows::settings_window::on_settings_close_requested`).
    pub const SETTINGS_CLOSING: &str = "dock://settings-closing";
}

/// Set once the user asked to quit; see the module docs.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Exit the application for real (tray *Quit*, `quit` command).
pub(crate) fn request_quit(app: &AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    app.exit(0);
}

pub fn run() {
    install_panic_hook();
    tauri::Builder::default()
        // Must be the first plugin: a second instance hands its arguments to
        // this one and exits before anything else is initialised.
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            log::info!("second instance started (args {args:?}, cwd {cwd:?}); revealing the dock");
            windows::reveal_all(app);
        }))
        .invoke_handler(commands::handler())
        .setup(|app| {
            // The platform decides where settings live (a mock never touches
            // the real file), so it is created first; it logs nothing until
            // `start`, which happens after logging is up.
            let platform = create_platform();
            let store = settings_store(app.handle(), platform.as_ref());
            let log_file = init_logging(app.handle(), store.as_ref());
            // Before the test panic, so that it leaves a crash marker like a
            // real one.
            crash::init(app.handle(), platform.as_ref());
            panic_test();
            log::info!("platform: {}", platform.name());

            let options = ServiceOptions {
                icon_cache_dir: icon_cache_dir(app.handle(), platform.as_ref()),
            };
            let demo = is_mock(platform.as_ref());
            let (service, settings_file) = start_service(platform, store, &options)?;
            let service = Arc::new(service);
            // The user's dock.css, read once here: before the dock
            // windows exist, so their first `get_user_css` has it.
            let user_css = user_css::UserCssState::new(settings_file.as_deref(), demo);
            user_css.reload();
            app.manage(user_css);
            forward_events(app.handle().clone(), &service);
            app.manage(AppService(Arc::clone(&service)));
            app.manage(AppPaths {
                settings_file,
                log_dir: app.path().app_log_dir().ok(),
                log_file,
            });
            app.manage(windows::DockGeometry::default());
            app.manage(windows::DockAutoHidden::default());
            app.manage(windows::DockFocus::default());
            app.manage(windows::SettingsWindowTheme::default());
            app.manage(appbar::AppBars::default());
            app.manage(backdrop::DockBackdropState::default());
            backdrop::init(app.handle(), &service);
            menus::init(app.handle())?;

            // Before the dock windows: their first panel reports ask for
            // the bands.
            appbar::init(app.handle(), &service.settings());
            windows::create_dock_windows(app.handle(), &service)?;
            if let Err(e) = tray::create(app.handle()) {
                log::error!("tray icon unavailable: {e}");
            }
            // Registered here rather than in the builder so that a plugin
            // that fails to start costs only its own feature.
            autostart::init(app.handle(), &service);
            taskbar::init(app.handle(), &service);
            shortcut::init(app.handle(), &service);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the Tauri application")
        .run(|app, event| match event {
            // No exit code means "the last window closed" (or similar user
            // interaction), which must not end a tray app. Programmatic
            // exits (`app.exit`, `request_restart`) carry a code.
            RunEvent::ExitRequested {
                code: None, api, ..
            } if !QUITTING.load(Ordering::SeqCst) => {
                log::debug!("exit requested by window close; staying in the tray");
                api.prevent_exit();
            }
            RunEvent::Exit => {
                log::info!("exiting");
                // Before the service goes: both talk to the platform. The
                // bands first, so the shell has no bar of ours to notify
                // when the taskbar's auto-hide changes back.
                appbar::on_exit(app);
                taskbar::on_exit(app);
                // After the restore: a clean exit is one that got this far.
                crash::on_exit();
                if let Some(service) = app.try_state::<AppService>() {
                    service.0.shutdown();
                }
            }
            _ => {}
        });
}

pub(crate) fn on_off(flag: bool) -> &'static str {
    if flag { "on" } else { "off" }
}
