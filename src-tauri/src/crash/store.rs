//! The marker store (the shape of `taskbar/marker.rs`).

use std::path::Path;

/// The marker as it is; a file that cannot be read or parsed counts as
/// absent, after a warning (the next write replaces it).
pub(super) fn read_marker<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            log::warn!(
                "crash: could not read the marker {} ({e}); counting it as absent",
                path.display()
            );
            return None;
        }
    };
    match serde_json::from_str::<T>(&text) {
        Ok(marker) => Some(marker),
        Err(e) => {
            log::warn!(
                "crash: the marker {} is not valid ({e}); counting it as absent",
                path.display()
            );
            None
        }
    }
}

/// Atomic (temp file + rename), like the settings file: a crash in the
/// middle must not leave a half-written marker that reads as absent.
pub(super) fn write_marker<T: serde::Serialize>(path: &Path, marker: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(marker)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Idempotent: a marker that is already gone is fine.
pub(super) fn delete_marker(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
