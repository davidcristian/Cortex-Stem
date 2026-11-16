use std::path::PathBuf;

use stem_core::AppFolder;

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
