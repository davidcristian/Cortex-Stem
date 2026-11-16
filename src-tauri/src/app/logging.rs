use std::path::{Path, PathBuf};

use stem_core::Settings;
use stem_service::{FileSettingsStore, SettingsStore};
use tauri::{AppHandle, Manager};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use crate::on_off;

/// File name (without extension) of the log file in the app log dir.
const LOG_FILE_NAME: &str = "stem";
/// The log file is truncated (`RotationStrategy::KeepOne`) past this size.
const LOG_MAX_BYTES: u128 = 2 * 1024 * 1024;

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
pub(crate) fn init_logging(app: &AppHandle, store: Option<&FileSettingsStore>) -> Option<PathBuf> {
    let raw_level = std::env::var("STEM_LOG").ok();
    let level = raw_level
        .as_deref()
        // Trimmed: cmd's `set STEM_LOG=debug && …` keeps the space.
        .and_then(|v| v.trim().parse::<log::LevelFilter>().ok())
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
pub(super) fn log_file_in(dir: &Path) -> PathBuf {
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
