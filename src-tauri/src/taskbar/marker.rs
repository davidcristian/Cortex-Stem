//! Where the marker is kept: `taskbar.json`, or memory under the mock
//! platform.

use std::fmt;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use stem_platform::rules::taskbar::Marker;

/// Where the marker is kept.
#[derive(Debug)]
pub(super) enum MarkerStore {
    File(PathBuf),
    /// The mock platform: in memory, so demo runs leave the disk alone.
    Memory(Mutex<Option<Marker>>),
}

/// The marker file's document.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerFile {
    previous_auto_hide: bool,
}

impl MarkerStore {
    /// The marker as it is; a file that cannot be read or parsed counts as
    /// absent, after a warning (the next write replaces it).
    pub(super) fn read(&self) -> Option<Marker> {
        match self {
            Self::File(path) => read_marker(path),
            Self::Memory(marker) => *marker.lock(),
        }
    }

    pub(super) fn write(&self, marker: Marker) -> std::io::Result<()> {
        match self {
            Self::File(path) => write_marker(path, marker),
            Self::Memory(slot) => {
                *slot.lock() = Some(marker);
                Ok(())
            }
        }
    }

    /// Idempotent: a marker that is already gone is fine.
    pub(super) fn delete(&self) -> std::io::Result<()> {
        match self {
            Self::File(path) => match std::fs::remove_file(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                result => result,
            },
            Self::Memory(slot) => {
                *slot.lock() = None;
                Ok(())
            }
        }
    }
}

impl fmt::Display for MarkerStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(path) => write!(f, "{}", path.display()),
            Self::Memory(_) => f.write_str("<memory>"),
        }
    }
}

fn read_marker(path: &Path) -> Option<Marker> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            log::warn!(
                "taskbar: could not read the marker {} ({e}); counting it as absent",
                path.display()
            );
            return None;
        }
    };
    match serde_json::from_str::<MarkerFile>(&text) {
        Ok(file) => Some(Marker {
            previous_auto_hide: file.previous_auto_hide,
        }),
        Err(e) => {
            log::warn!(
                "taskbar: the marker {} is not valid ({e}); counting it as absent",
                path.display()
            );
            None
        }
    }
}

/// Atomic (temp file + rename), like the settings file: a crash in the
/// middle must not leave a half-written marker that reads as absent.
fn write_marker(path: &Path, marker: Marker) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&MarkerFile {
        previous_auto_hide: marker.previous_auto_hide,
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}
