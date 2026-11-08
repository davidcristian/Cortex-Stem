//! Hide the Windows taskbar: `system.hideWindowsTaskbar` turns on
//! Windows' own *Automatically hide the taskbar* while Stem runs and puts
//! back what the user had when Stem exits.
//!
//! Nothing hides `Shell_TrayWnd`. The taskbar is set to auto-hide through
//! `SHAppBarMessage(ABM_SETSTATE, ABS_AUTOHIDE)`
//! (`Platform::set_taskbar_auto_hide`), which is reversible, which the
//! Taskbar settings page shows, and which survives an Explorer restart.
//! Windows persists it, so the state the user had is remembered here: the
//! **marker** `taskbar.json` (`{"previousAutoHide": bool}`) in the app's
//! local data folder (`%LOCALAPPDATA%\dev.davidcristian.stem\`, the logs'
//! parent), written before the first change and deleted after the restore.
//! A marker found at start means the previous run did not restore (a crash,
//! *End task*): the state it holds is still the one to go back to. The
//! decision is [`stem_platform::rules::taskbar`] (pure, tested on every
//! host); this module is its I/O, performing a [`Plan`] in field order. A
//! marker that cannot be written leaves the taskbar alone (a state that
//! cannot be restored is not changed); a set that fails is logged at `warn`
//! and keeps the marker, so the next start retries. A live state that
//! cannot be read (no tray: Explorer crashed or is restarting; or Explorer
//! did not answer in time) never writes a new marker and never deletes the
//! one there is: the plan may still attempt the set, and the marker stays
//! for the next start.
//!
//! Syncs run at start and whenever a settings change flips the value, as in
//! `autostart`: `SHAppBarMessage` waits on Explorer, so each sync is a task
//! on Tauri's blocking pool (never the main thread, never the service
//! worker), the tasks run one at a time, and each applies the latest wanted
//! value, so the last change wins in whatever order the tasks start. The
//! exit restore ([`on_exit`]) runs inline on the thread that ends the app:
//! the process ends right after, so there is no later moment for it, and
//! a sync task still queued at that point dies with the process. It cannot
//! hang the quit: the platform bounds each call's wait on Explorer to about
//! 500 ms (`stem_platform::windows::taskbar`), so the restore returns within
//! about 1 s (one read, at most one set), within about 500 ms when Explorer
//! is hung outright (the calls after a timeout fail at once); only a sync
//! still running when Quit is chosen (right after a settings change) is
//! waited for first, which can add up to about 1 s more.
//!
//! The mock platform (demo runs) keeps the marker in memory instead of on
//! disk, so nothing real is touched while the whole flow still runs against
//! the mock's scripted taskbar.
//!
//! Not tracked: the user flipping Windows' own setting while Stem runs; the
//! restore puts back the marker's state.

use std::fmt;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use stem_core::Settings;
use stem_platform::rules::taskbar::{Marker, Plan, plan, plan_exit};
use stem_platform::{Platform, is_mock};
use stem_service::DockService;
use tauri::{AppHandle, Manager};

use crate::{AppService, on_off};

/// The marker's file name, in `app_local_data_dir()`.
const MARKER_FILE_NAME: &str = "taskbar.json";

/// Managed state; it exists once `init` knows where the marker lives.
#[derive(Debug)]
struct TaskbarSync {
    /// What the latest settings ask for; `None` before the first request.
    wanted: Mutex<Option<bool>>,
    /// Held while a sync (or the exit restore) runs, so that they never
    /// overlap.
    running: Mutex<()>,
    marker: MarkerStore,
}

/// Where the marker is kept.
#[derive(Debug)]
enum MarkerStore {
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

/// Find where the marker lives and bring the taskbar in line with the
/// settings. Runs in `setup`, after `forward_events`: the settings are read
/// once the state is managed, so a change in between is not lost. Without
/// an app local data dir there is nowhere to remember the user's state, so
/// the setting has no effect this session and the reason is logged once.
pub fn init(app: &AppHandle, service: &DockService) {
    let marker = if is_mock(service.platform().as_ref()) {
        log::info!("taskbar: the mock platform keeps the marker in memory");
        MarkerStore::Memory(Mutex::new(None))
    } else {
        match app.path().app_local_data_dir() {
            Ok(dir) => MarkerStore::File(dir.join(MARKER_FILE_NAME)),
            Err(e) => {
                log::warn!(
                    "taskbar: no app local data dir ({e}); Hide the Windows taskbar has no effect this session"
                );
                return;
            }
        }
    };
    app.manage(TaskbarSync {
        wanted: Mutex::new(None),
        running: Mutex::new(()),
        marker,
    });
    on_settings_changed(app, &service.settings());
}

/// The settings changed (or the app started): schedule a sync when
/// `system.hideWindowsTaskbar` differs from the previous request. Any
/// thread; never blocks.
pub fn on_settings_changed(app: &AppHandle, settings: &Settings) {
    let Some(sync) = app.try_state::<TaskbarSync>() else {
        return;
    };
    let wanted = settings.system.hide_windows_taskbar;
    {
        let mut requested = sync.wanted.lock();
        if *requested == Some(wanted) {
            return;
        }
        *requested = Some(wanted);
    }
    let app = app.clone();
    drop(tauri::async_runtime::spawn_blocking(move || {
        let (Some(sync), Some(service)) = (
            app.try_state::<TaskbarSync>(),
            app.try_state::<AppService>(),
        ) else {
            return;
        };
        let _one_at_a_time = sync.running.lock();
        // The latest request, not necessarily the one that spawned this task.
        let Some(wanted) = *sync.wanted.lock() else {
            return;
        };
        let platform = service.0.platform();
        let plan = plan(wanted, sync.marker.read(), live_state(platform.as_ref()));
        log::debug!("taskbar: setting {}: {plan:?}", on_off(wanted));
        sync.apply(platform.as_ref(), plan);
    }));
}

/// Put the taskbar back as the user had it (`RunEvent::Exit`, the real quit
/// path). Runs **inline** on the calling thread, unlike the syncs: the
/// process ends right after this, so a task on the blocking pool would never
/// run, and a bounded wait on Explorer is acceptable on the way out (about
/// 1 s at most, see the module documentation). Waits for a sync in flight,
/// so the two never interleave. With the live state unreadable the restore
/// is attempted and the marker stays for the next start.
pub fn on_exit(app: &AppHandle) {
    let (Some(sync), Some(service)) = (
        app.try_state::<TaskbarSync>(),
        app.try_state::<AppService>(),
    ) else {
        return;
    };
    let _one_at_a_time = sync.running.lock();
    let platform = service.0.platform();
    let plan = plan_exit(sync.marker.read(), live_state(platform.as_ref()));
    log::debug!("taskbar: exit: {plan:?}");
    sync.apply(platform.as_ref(), plan);
}

/// The taskbar's auto-hide state now; `None`, after a warning, when it
/// cannot be read (no tray, or no answer in time): the plan then neither
/// writes nor deletes the marker, and sets without comparing only when
/// there is a marker to restore from.
fn live_state(platform: &dyn Platform) -> Option<bool> {
    match platform.taskbar_auto_hide() {
        Ok(state) => Some(state),
        Err(e) => {
            log::warn!(
                "taskbar: could not read whether the taskbar auto-hides ({e}); the marker is \
                 neither written nor deleted this time"
            );
            None
        }
    }
}

impl TaskbarSync {
    /// Perform `plan` in its field order. Blocking pool, or the exit thread.
    fn apply(&self, platform: &dyn Platform, plan: Plan) {
        if plan.is_nothing() {
            log::debug!("taskbar: nothing to do");
            return;
        }
        if let Some(marker) = plan.write_marker {
            if let Err(e) = self.marker.write(marker) {
                log::warn!(
                    "taskbar: could not write the marker {} ({e}); leaving the taskbar alone",
                    self.marker
                );
                return;
            }
            log::info!(
                "taskbar: remembered in {} that auto-hide was {} before Stem",
                self.marker,
                on_off(marker.previous_auto_hide)
            );
        }
        if let Some(enabled) = plan.set_auto_hide {
            match platform.set_taskbar_auto_hide(enabled) {
                Ok(()) => log::info!("taskbar auto-hide {}", on_off(enabled)),
                Err(e) => {
                    log::warn!(
                        "taskbar: could not turn auto-hide {} ({e}); the marker stays so the next start retries",
                        on_off(enabled)
                    );
                    return;
                }
            }
        }
        if plan.delete_marker {
            match self.marker.delete() {
                Ok(()) => log::info!("taskbar: the user's state is back; marker deleted"),
                Err(e) => log::warn!(
                    "taskbar: could not delete the marker {} ({e}); the next start will restore again",
                    self.marker
                ),
            }
        }
    }
}

impl MarkerStore {
    /// The marker as it is; a file that cannot be read or parsed counts as
    /// absent, after a warning (the next write replaces it).
    fn read(&self) -> Option<Marker> {
        match self {
            Self::File(path) => read_marker(path),
            Self::Memory(marker) => *marker.lock(),
        }
    }

    fn write(&self, marker: Marker) -> std::io::Result<()> {
        match self {
            Self::File(path) => write_marker(path, marker),
            Self::Memory(slot) => {
                *slot.lock() = Some(marker);
                Ok(())
            }
        }
    }

    /// Idempotent: a marker that is already gone is fine.
    fn delete(&self) -> std::io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stem-taskbar-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_marker_file_round_trips_in_the_documented_shape() {
        let dir = scratch_dir("round-trip");
        let store = MarkerStore::File(dir.join("nested").join(MARKER_FILE_NAME));
        assert_eq!(store.read(), None);
        store
            .write(Marker {
                previous_auto_hide: true,
            })
            .unwrap();
        let text = std::fs::read_to_string(dir.join("nested").join(MARKER_FILE_NAME)).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            serde_json::json!({ "previousAutoHide": true })
        );
        assert!(!dir.join("nested").join("taskbar.json.tmp").exists());
        assert_eq!(
            store.read(),
            Some(Marker {
                previous_auto_hide: true,
            })
        );
        store.delete().unwrap();
        assert_eq!(store.read(), None);
        store.delete().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_invalid_marker_file_counts_as_absent() {
        let dir = scratch_dir("invalid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(MARKER_FILE_NAME);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(MarkerStore::File(path).read(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_memory_store_holds_one_marker_and_touches_no_file() {
        let store = MarkerStore::Memory(Mutex::new(None));
        assert_eq!(store.read(), None);
        store
            .write(Marker {
                previous_auto_hide: false,
            })
            .unwrap();
        assert_eq!(
            store.read(),
            Some(Marker {
                previous_auto_hide: false,
            })
        );
        store.delete().unwrap();
        assert_eq!(store.read(), None);
        assert_eq!(store.to_string(), "<memory>");
    }
}
