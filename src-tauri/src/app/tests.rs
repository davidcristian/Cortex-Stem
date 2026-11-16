use std::path::{Path, PathBuf};

use stem_core::AppFolder;

use super::logging::log_file_in;
use super::paths::AppPaths;

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
