//! The Tauri shell. Thin by design: every decision lives in `stem-service`
//! and below. This crate only
//! - creates and positions the dock window(s) and the settings window,
//! - exposes `stem-service` through Tauri commands (`commands`),
//! - forwards `ServiceEvent`s as Tauri events (`docs/ipc/events.md`),
//! - wires plugins (single-instance, logging, autostart, global shortcut)
//!   and the tray icon, and applies the settings that belong to the shell
//!   (`autostart`, `shortcut`, click-through in `windows`, the tray's check
//!   mark).
//!
//! Lifecycle: this is a tray application. The dock window ignores close
//! requests, closing the settings window only destroys that window, and an
//! exit requested "by user interaction" (`RunEvent::ExitRequested` with no
//! code) is refused unless [`request_quit`] was called (tray *Quit* or the
//! `quit` command).

mod autostart;
mod commands;
mod menus;
mod shortcut;
mod tray;
mod windows;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use stem_core::Settings;
use stem_platform::{Platform, create_platform, is_mock};
use stem_service::{
    DockService, FileSettingsStore, MemorySettingsStore, ServiceError, ServiceEvent, ServiceResult,
    SettingsStore,
};
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

/// Shared handle to the service, stored in Tauri's managed state.
#[derive(Debug, Clone)]
pub struct AppService(pub Arc<DockService>);

/// Event names, as documented in `docs/ipc/events.md`.
pub mod events {
    pub const STATE: &str = "dock://state";
    pub const SETTINGS: &str = "dock://settings";
    pub const THEME: &str = "dock://theme";
    pub const MONITORS: &str = "dock://monitors";
    pub const BOUNCE: &str = "dock://item-bounce";
}

/// File name (without extension) of the log file in the app log dir.
const LOG_FILE_NAME: &str = "dock";
/// The log file is truncated (`RotationStrategy::KeepOne`) past this size.
const LOG_MAX_BYTES: u128 = 2 * 1024 * 1024;

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
            init_logging(app.handle(), store.as_ref());
            panic_test();
            log::info!("platform: {}", platform.name());

            let service = Arc::new(start_service(platform, store)?);
            forward_events(app.handle().clone(), &service);
            app.manage(AppService(Arc::clone(&service)));
            app.manage(windows::DockGeometry::default());
            menus::init(app.handle())?;

            windows::create_dock_windows(app.handle(), &service)?;
            if let Err(e) = tray::create(app.handle()) {
                log::error!("tray icon unavailable: {e}");
            }
            // Registered here rather than in the builder so that a plugin
            // that fails to start costs only its own feature.
            autostart::init(app.handle(), &service);
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
                if let Some(service) = app.try_state::<AppService>() {
                    service.0.shutdown();
                }
            }
            _ => {}
        });
}

// ---------------------------------------------------------------------------
// Settings persistence
// ---------------------------------------------------------------------------

/// Settings live in `<app config dir>/settings.json`. `None` means an
/// in-memory store: the mock platform (so demo runs never touch a real
/// profile, however the mock was chosen) or a host without a config dir.
fn settings_store(app: &AppHandle, platform: &dyn Platform) -> Option<FileSettingsStore> {
    if is_mock(platform) {
        return None;
    }
    match app.path().app_config_dir() {
        Ok(dir) => Some(FileSettingsStore::new(dir.join("settings.json"))),
        Err(e) => {
            log::error!("no app config dir ({e}); settings will not persist");
            None
        }
    }
}

/// Start the service, never failing because of the settings *file*:
/// - no file yet: write the defaults first so users can find and edit it;
/// - unreadable or invalid file: keep it as `settings.json.broken`, write
///   fresh defaults and start with them;
/// - unreadable directory: run with an in-memory store for this session.
fn start_service(
    platform: Arc<dyn Platform>,
    store: Option<FileSettingsStore>,
) -> ServiceResult<DockService> {
    let Some(file) = store else {
        return DockService::start(platform, Box::new(MemorySettingsStore::default()));
    };
    log::info!("settings file: {}", file.path().display());
    write_defaults_if_missing(&file);

    // `DockService::start` loads and parses the document before it touches
    // the platform, so a settings error leaves the platform unstarted and a
    // second attempt is safe.
    match DockService::start(Arc::clone(&platform), Box::new(file.clone())) {
        Err(ServiceError::Settings(e)) => {
            let broken = file.path().with_extension("json.broken");
            log::error!(
                "settings file {} cannot be used ({e}); moving it to {} and starting with defaults",
                file.path().display(),
                broken.display()
            );
            match std::fs::rename(file.path(), &broken) {
                Ok(()) => {
                    write_defaults_if_missing(&file);
                    DockService::start(platform, Box::new(file))
                }
                Err(io) => {
                    log::error!(
                        "could not move the broken settings file ({io}); settings will not persist this session"
                    );
                    DockService::start(platform, Box::new(MemorySettingsStore::default()))
                }
            }
        }
        Err(ServiceError::Store(e)) => {
            log::error!("settings cannot be read ({e}); settings will not persist this session");
            DockService::start(platform, Box::new(MemorySettingsStore::default()))
        }
        other => other,
    }
}

fn write_defaults_if_missing(file: &FileSettingsStore) {
    if file.path().exists() {
        return;
    }
    match file.save(&Settings::default().to_json_pretty()) {
        Ok(()) => log::info!("wrote default settings to {}", file.path().display()),
        Err(e) => log::warn!("could not write the default settings file: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Logging and panic handling
// ---------------------------------------------------------------------------

/// Register `tauri-plugin-log`: stdout always, plus `<app log dir>/stem.log`
/// when `settings.system.fileLogging` is on *in the file as it is at
/// startup* (changing the setting takes effect at the next start). The
/// level comes from `STEM_LOG` (`trace|debug|info|warn|error`, default
/// `info`).
///
/// Runs inside `setup` rather than in the builder because the setting is
/// read from the app config dir, which needs an `AppHandle`. Anything logged
/// before this point (only the panic hook can) is dropped by `log`.
fn init_logging(app: &AppHandle, store: Option<&FileSettingsStore>) {
    let raw_level = std::env::var("STEM_LOG").ok();
    let level = raw_level
        .as_deref()
        .and_then(|v| v.parse::<log::LevelFilter>().ok())
        .unwrap_or(log::LevelFilter::Info);
    let file_logging = store.is_none_or(file_logging_wanted);

    let registered = match app.plugin(log_plugin(level, file_logging)) {
        Ok(()) => Ok(file_logging),
        // Most likely the log directory could not be created or written:
        // keep at least stdout so the failure is visible.
        Err(e) if file_logging => app.plugin(log_plugin(level, false)).map(|()| {
            log::error!("file logging unavailable ({e}); logging to stdout only");
            false
        }),
        Err(e) => Err(e),
    };
    match registered {
        Ok(file_logging) => {
            log::info!("log level {level}; file logging {}", on_off(file_logging));
            if file_logging {
                match app.path().app_log_dir() {
                    Ok(dir) => log::info!(
                        "log file: {}",
                        dir.join(format!("{LOG_FILE_NAME}.log")).display()
                    ),
                    Err(e) => log::warn!("app log dir unknown: {e}"),
                }
            }
        }
        // No logger is installed; this line reaches nobody, but there is no
        // better channel left and the app still works.
        Err(e) => log::error!("logging unavailable: {e}"),
    }
    if let Some(raw) = raw_level
        && raw.parse::<log::LevelFilter>().is_err()
    {
        log::warn!("STEM_LOG={raw:?} is not a log level; using info");
    }
}

fn log_plugin(
    level: log::LevelFilter,
    file_logging: bool,
) -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let mut builder = tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::Stdout))
        .level(level)
        .rotation_strategy(RotationStrategy::KeepOne)
        .max_file_size(LOG_MAX_BYTES);
    if file_logging {
        builder = builder.target(Target::new(TargetKind::LogDir {
            file_name: Some(LOG_FILE_NAME.to_owned()),
        }));
    }
    builder.build()
}

/// `system.fileLogging` from the settings file as it is right now. Defaults
/// to on when the file is missing or unreadable (the service reports that
/// separately).
fn file_logging_wanted(store: &FileSettingsStore) -> bool {
    match store.load() {
        Ok(Some(json)) => Settings::from_json(&json)
            .map(|s| s.system.file_logging)
            .unwrap_or(true),
        _ => true,
    }
}

pub(crate) fn on_off(flag: bool) -> &'static str {
    if flag { "on" } else { "off" }
}

/// Log every panic (message, location, backtrace) before the default hook
/// prints it. With `panic = "abort"` in release builds this is the only
/// record of a crash, so it goes through `log` (file + stdout; the file
/// target flushes per record).
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<non-string panic payload>");
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_owned());
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("panic at {location}: {message}\nbacktrace:\n{backtrace}");
        default_hook(info);
    }));
}

/// Debug builds only: `STEM_PANIC_TEST=1` panics once logging is up, so the
/// panic hook can be exercised.
fn panic_test() {
    if cfg!(debug_assertions) && std::env::var("STEM_PANIC_TEST").is_ok_and(|v| v == "1") {
        panic!("STEM_PANIC_TEST=1: intentional panic to exercise the panic hook");
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Turn service events into Tauri events for every window. Runs on the
/// service worker thread; `emit` is thread-safe and the dock re-layout
/// hops to the main thread itself.
fn forward_events(app: AppHandle, service: &DockService) {
    service.subscribe(Box::new(move |event| {
        let result = match event {
            ServiceEvent::State(state) => app.emit(events::STATE, state),
            ServiceEvent::Settings(settings) => {
                windows::on_settings_changed(&app, settings);
                tray::on_settings_changed(&app, settings);
                autostart::on_settings_changed(&app, settings);
                shortcut::on_settings_changed(&app, settings);
                app.emit(events::SETTINGS, settings)
            }
            ServiceEvent::Theme(theme) => app.emit(events::THEME, theme),
            ServiceEvent::Monitors(monitors) => {
                windows::on_monitors_changed(&app, monitors.clone());
                app.emit(events::MONITORS, monitors)
            }
            ServiceEvent::Bounce { item_key } => {
                app.emit(events::BOUNCE, BouncePayload { item_key })
            }
        };
        if let Err(e) = result {
            log::warn!("failed to emit event: {e}");
        }
    }));
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BouncePayload<'a> {
    item_key: &'a str,
}
