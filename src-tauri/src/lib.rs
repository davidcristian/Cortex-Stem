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
//!   About section.
//!
//! Lifecycle: this is a tray application. The dock window ignores close
//! requests, closing the settings window only destroys that window, and an
//! exit requested "by user interaction" (`RunEvent::ExitRequested` with no
//! code) is refused unless [`request_quit`] was called (tray *Quit* or the
//! `quit` command).

mod appbar;
mod autostart;
mod backdrop;
mod commands;
mod crash;
mod menus;
mod shortcut;
mod taskbar;
mod tray;
mod windows;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use stem_core::{AppFolder, Settings};
use stem_platform::{Platform, create_platform, is_mock};
use stem_service::{
    DockService, FileSettingsStore, MemorySettingsStore, ServiceError, ServiceEvent,
    ServiceOptions, ServiceResult, SettingsStore,
};
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

/// Shared handle to the service, stored in Tauri's managed state.
#[derive(Debug, Clone)]
pub struct AppService(pub Arc<DockService>);

/// Where this installation keeps its files, resolved once in
/// `setup` and kept in Tauri's managed state for `commands::get_app_info`
/// and `commands::open_app_folder`: the settings page's About section shows
/// the paths and opens the folders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppPaths {
    /// The settings file the service reads and writes; `None` under the
    /// in-memory store (the mock platform, or a profile that could not be
    /// used this session, see `start_service`).
    pub settings_file: Option<PathBuf>,
    /// The app's log folder (`app_log_dir()`), whether or not it exists
    /// yet; `None` when the host has none.
    pub log_dir: Option<PathBuf>,
    /// `stem.log` in `log_dir` while file logging is on this run; `None`
    /// when it is off (`system.fileLogging` at startup) or unavailable.
    pub log_file: Option<PathBuf>,
}

impl AppPaths {
    /// The folder `open_app_folder` opens for `folder`: the log folder; or,
    /// while none exists yet (file logging has never been on), its parent,
    /// the app's local data folder, which the crash markers and
    /// `taskbar.json` share and which exists once the app has run; the
    /// settings file's folder. `Err` is the command's error string: a demo
    /// run has no settings file, and a host without the directories has
    /// nothing to open. Touches the disk (existence checks), so it runs on
    /// the blocking pool.
    pub fn folder_to_open(&self, folder: AppFolder) -> Result<PathBuf, String> {
        match folder {
            AppFolder::Logs => {
                let dir = self
                    .log_dir
                    .as_deref()
                    .ok_or_else(|| "the log folder is unknown on this host".to_owned())?;
                if dir.is_dir() {
                    return Ok(dir.to_path_buf());
                }
                match dir.parent().filter(|parent| parent.is_dir()) {
                    Some(parent) => {
                        log::info!(
                            "the log folder {} does not exist yet; opening {} instead",
                            dir.display(),
                            parent.display()
                        );
                        Ok(parent.to_path_buf())
                    }
                    None => Err(format!("the log folder {} does not exist", dir.display())),
                }
            }
            AppFolder::Settings => {
                let file = self
                    .settings_file
                    .as_deref()
                    .ok_or_else(|| "settings are not saved to a file in this run".to_owned())?;
                let dir = file
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .ok_or_else(|| format!("{} has no folder", file.display()))?;
                if !dir.is_dir() {
                    return Err(format!(
                        "the settings folder {} does not exist",
                        dir.display()
                    ));
                }
                Ok(dir.to_path_buf())
            }
        }
    }
}

/// Event names, as documented in `docs/ipc/events.md`. All but `SHOW`, `FOCUS`
/// and `RELEASE_FOCUS` forward a `ServiceEvent`; those come from the shell
/// itself (`windows::toggle_dock`, `windows::on_focus_shortcut`).
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
}

/// File name (without extension) of the log file in the app log dir.
const LOG_FILE_NAME: &str = "stem";
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
            let log_file = init_logging(app.handle(), store.as_ref());
            // Before the test panic, so that it leaves a crash marker like a
            // real one.
            crash::init(app.handle(), platform.as_ref());
            panic_test();
            log::info!("platform: {}", platform.name());

            let options = ServiceOptions {
                icon_cache_dir: icon_cache_dir(app.handle(), platform.as_ref()),
            };
            let (service, settings_file) = start_service(platform, store, &options)?;
            let service = Arc::new(service);
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

/// The disk icon cache lives in `<app local data dir>/icons`
/// (`%LOCALAPPDATA%\dev.davidcristian.stem\icons`). `None` under the
/// mock platform (a demo run never touches the real profile; its keys are
/// never cached anyway) or on a host without that directory, which leaves
/// every icon to the platform.
fn icon_cache_dir(app: &AppHandle, platform: &dyn Platform) -> Option<PathBuf> {
    if is_mock(platform) {
        return None;
    }
    match app.path().app_local_data_dir() {
        Ok(dir) => Some(dir.join("icons")),
        Err(e) => {
            log::warn!("no app local data dir ({e}); icons will not be cached on disk");
            None
        }
    }
}

/// Start the service, never failing because of the settings *file*:
/// - no file yet: write the defaults first so users can find and edit it;
/// - unreadable or invalid file: keep it as `settings.json.broken`, write
///   fresh defaults and start with them;
/// - unreadable directory: run with an in-memory store for this session.
///
/// `options` (the icon cache directory) apply whichever store it ends up
/// with. Also returns the settings file the service ended up with (`None`
/// for the in-memory store), for `AppPaths`.
fn start_service(
    platform: Arc<dyn Platform>,
    store: Option<FileSettingsStore>,
    options: &ServiceOptions,
) -> ServiceResult<(DockService, Option<PathBuf>)> {
    let Some(file) = store else {
        return in_memory(platform, options);
    };
    let path = file.path().to_path_buf();
    log::info!("settings file: {}", path.display());
    write_defaults_if_missing(&file);

    // `DockService::start` loads and parses the document before it touches
    // the platform, so a settings error leaves the platform unstarted and a
    // second attempt is safe.
    match DockService::start_with(
        Arc::clone(&platform),
        Box::new(file.clone()),
        options.clone(),
    ) {
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
                    DockService::start_with(platform, Box::new(file), options.clone())
                        .map(|s| (s, Some(path)))
                }
                Err(io) => {
                    log::error!(
                        "could not move the broken settings file ({io}); settings will not persist this session"
                    );
                    in_memory(platform, options)
                }
            }
        }
        Err(ServiceError::Store(e)) => {
            log::error!("settings cannot be read ({e}); settings will not persist this session");
            in_memory(platform, options)
        }
        other => other.map(|s| (s, Some(path))),
    }
}

/// The service over an in-memory store: nothing is saved, so there is no
/// settings file to report.
fn in_memory(
    platform: Arc<dyn Platform>,
    options: &ServiceOptions,
) -> ServiceResult<(DockService, Option<PathBuf>)> {
    DockService::start_with(
        platform,
        Box::new(MemorySettingsStore::default()),
        options.clone(),
    )
    .map(|s| (s, None))
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
///
/// Returns the log file while file logging is on and the log folder is
/// known (for `AppPaths`), else `None`.
fn init_logging(app: &AppHandle, store: Option<&FileSettingsStore>) -> Option<PathBuf> {
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
    let mut log_file = None;
    match registered {
        Ok(file_logging) => {
            log::info!("log level {level}; file logging {}", on_off(file_logging));
            if file_logging {
                match app.path().app_log_dir() {
                    Ok(dir) => {
                        let file = log_file_in(&dir);
                        log::info!("log file: {}", file.display());
                        log_file = Some(file);
                    }
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
    log_file
}

/// The file `tauri-plugin-log`'s `LogDir` target writes in `dir`.
fn log_file_in(dir: &Path) -> PathBuf {
    dir.join(format!("{LOG_FILE_NAME}.log"))
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

/// Write the crash marker (`crash::record_panic`, so the next start can
/// report the crash) and log every panic (message, location,
/// backtrace) before the default hook prints it. With `panic = "abort"` in
/// release builds these are the only record of a crash, so the log line
/// goes through `log` (file + stdout; the file target flushes per record).
/// The marker comes first: it is one small write, and the next start shows
/// it even if the log line never makes it out.
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
        crash::record_panic(message, &location);
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("panic at {location}: {message}\nbacktrace:\n{backtrace}");
        default_hook(info);
    }));
}

/// Debug builds only: `STEM_PANIC_TEST=1` panics once logging is up, so the
/// panic hook can be exercised and the next start's crash report with it.
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
                tray::on_settings_changed(&app, settings);
                autostart::on_settings_changed(&app, settings);
                taskbar::on_settings_changed(&app, settings);
                shortcut::on_settings_changed(&app, settings);
                // The pages before their windows: the emit is queued on the
                // main thread ahead of the window work `windows` posts, so
                // on a new edge a page usually lays itself out for
                // it before its window moves, and its next content-size
                // report is read for the edge it was laid out for.
                let emitted = app.emit(events::SETTINGS, settings);
                windows::on_settings_changed(&app, settings);
                // After `windows`: a position change moves the windows
                // first, and the bands follow them.
                appbar::on_settings_changed(&app, settings);
                // The backdrop prototype follows the setting and the theme.
                backdrop::on_settings_changed(&app, settings);
                emitted
            }
            ServiceEvent::Theme(theme) => {
                backdrop::on_theme_changed(&app, theme);
                app.emit(events::THEME, theme)
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stem-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_log_folder_opens_when_it_exists_and_its_parent_until_then() {
        let data = scratch_dir("logs");
        let logs = data.join("logs");
        let paths = AppPaths {
            log_dir: Some(logs.clone()),
            ..AppPaths::default()
        };
        assert_eq!(paths.folder_to_open(AppFolder::Logs), Ok(data.clone()));
        std::fs::create_dir_all(&logs).unwrap();
        assert_eq!(paths.folder_to_open(AppFolder::Logs), Ok(logs.clone()));

        assert_eq!(
            AppPaths::default().folder_to_open(AppFolder::Logs),
            Err("the log folder is unknown on this host".to_owned())
        );
        let gone = AppPaths {
            log_dir: Some(data.join("missing").join("logs")),
            ..AppPaths::default()
        };
        assert!(
            gone.folder_to_open(AppFolder::Logs)
                .is_err_and(|e| e.starts_with("the log folder ") && e.ends_with(" does not exist")),
            "neither the folder nor its parent exists"
        );
        std::fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn the_settings_folder_is_the_files_parent_and_a_demo_run_has_none() {
        let config = scratch_dir("settings");
        let paths = AppPaths {
            settings_file: Some(config.join("settings.json")),
            ..AppPaths::default()
        };
        assert_eq!(
            paths.folder_to_open(AppFolder::Settings),
            Ok(config.clone())
        );

        assert_eq!(
            AppPaths::default().folder_to_open(AppFolder::Settings),
            Err("settings are not saved to a file in this run".to_owned())
        );
        let unwritten = AppPaths {
            settings_file: Some(config.join("never-made").join("settings.json")),
            ..AppPaths::default()
        };
        assert!(
            unwritten
                .folder_to_open(AppFolder::Settings)
                .is_err_and(|e| e.starts_with("the settings folder ")),
            "a folder the first save never created"
        );
        std::fs::remove_dir_all(&config).unwrap();
    }

    #[test]
    fn the_log_file_is_stem_log_in_the_log_folder() {
        assert_eq!(
            log_file_in(Path::new("logs")),
            Path::new("logs").join("stem.log")
        );
    }
}
