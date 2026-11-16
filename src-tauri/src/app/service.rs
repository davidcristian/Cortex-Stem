//! Starting the service: the settings store and the icon cache folder
//! it runs with, and what happens when the settings file cannot be used.

use std::path::PathBuf;
use std::sync::Arc;

use stem_core::Settings;
use stem_platform::{Platform, is_mock};
use stem_service::store::StoreError;
use stem_service::{
    DockService, FileSettingsStore, MemorySettingsStore, ServiceError, ServiceOptions,
    ServiceResult, SettingsStore,
};
use tauri::{AppHandle, Manager};

/// Settings live in `<app config dir>/settings.json`. `None` means an
/// in-memory store: the mock platform (so demo runs never touch a real
/// profile, however the mock was chosen) or a host without a config dir.
pub(crate) fn settings_store(
    app: &AppHandle,
    platform: &dyn Platform,
) -> Option<FileSettingsStore> {
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
pub(crate) fn icon_cache_dir(app: &AppHandle, platform: &dyn Platform) -> Option<PathBuf> {
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
/// - a file that is not text (`StoreError::Encoding`) or not a valid
///   document: keep it as `settings.json.broken`, write fresh defaults and
///   start with them (UTF-8 with a byte order mark and UTF-16 are text);
/// - a file or directory that cannot be read: run with an in-memory store
///   for this session.
///
/// `options` (the icon cache directory) apply whichever store it ends up
/// with. Also returns the settings file the service ended up with (`None`
/// for the in-memory store), for `AppPaths`.
pub(crate) fn start_service(
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
        Err(e @ (ServiceError::Settings(_) | ServiceError::Store(StoreError::Encoding { .. }))) => {
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
