//! Crash reporting: after a panic, the next start tells the user
//! once, in a native dialog, and offers to open the log folder; an unclean
//! exit that was not a panic is only logged.
//!
//! Two **markers** live in the app's local data folder
//! (`%LOCALAPPDATA%\dev.davidcristian.stem\`, the logs' parent, next to
//! `taskbar.json`):
//! - `run.json` (`{"startedAt": "<RFC 3339 UTC>", "pid": n, "version":
//!   "<package version>"}`) is written at start, right after logging is up,
//!   and deleted at `RunEvent::Exit` after the taskbar restore. One found at
//!   the next start, with another process's pid, means the previous run
//!   never reached `Exit`: a kill (*End task*), a power loss, possibly a
//!   sign-out. That is not a crash, so it is logged once
//!   at `warn` and nothing else happens.
//! - `crash.json` (`{"kind": "panic", "message": "…", "location":
//!   "file:line:col", "time": "<RFC 3339 UTC>"}`) is written by the panic
//!   hook ([`record_panic`], called by `install_panic_hook` in `lib.rs`
//!   before its log line). One found at the next start is shown to the user
//!   in a native, topmost *Yes* / *No* box (`Platform::ask_yes_no`) that
//!   says when, where and what, and offers to open the log folder; *Yes*
//!   opens it in Explorer (`Platform::launch` on the folder path). Whatever
//!   the answer, and whether or not the box could be shown, the marker is
//!   deleted: one dialog per crash. A crash marker outranks a run marker
//!   (the crash is the reason the run did not exit cleanly).
//!
//! The hook has no `AppHandle`, so [`init`] resolves the folder once into
//! [`MARKER_DIR`]; a panic before that (before logging is up) writes no
//! marker, just as it logs nothing. Writing the marker is best effort:
//! nothing in that path can panic (a panic inside the hook aborts the
//! process), a failure is logged and the hook goes on to its log line.
//! Release builds have `panic = "abort"`, which makes the hook the last
//! thing that runs: the marker and the log line are the only record of a
//! crash. In debug builds a panic on a worker thread unwinds that thread
//! only, so the marker may be written while the app lives on; the next
//! start still reports it, which is right (a panic is a bug either way).
//!
//! What the leftovers mean is the pure [`assess`], and what the box says is
//! the pure [`dialog_text`]; both are unit-tested here, the rest is I/O.
//! The leftovers are read and this run's marker is written in `setup`
//! (small local files, like the settings file); only the report is
//! scheduled, 2 s later on Tauri's blocking pool, so the dock is on screen
//! before the box and `MessageBoxW`, which blocks until answered, never
//! runs on the UI thread or the service worker
//! (`docs/development/windows-rules.md`). The store is the shape of
//! `taskbar/marker.rs` (atomic temp file + rename, an unreadable file counts
//! as absent after a warning), copied rather than shared: this module has no
//! in-memory variant, because under the mock platform (demo runs) nothing
//! is written and the check does nothing, like autostart and taskbar. No
//! external crate for the time stamp: [`rfc3339_utc`] formats a
//! `SystemTime` by hand.

mod markers;
mod store;
#[cfg(test)]
mod tests;
mod timestamp;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use stem_core::LaunchTarget;
use stem_platform::{Platform, is_mock};
use tauri::{AppHandle, Manager};

use markers::{CrashKind, CrashMarker, RunMarker, Verdict, assess, dialog_text};
use store::{delete_marker, read_marker, write_marker};
use timestamp::rfc3339_utc;

use crate::AppService;

/// The run marker's file name, in `app_local_data_dir()`.
const RUN_FILE_NAME: &str = "run.json";
/// The crash marker's file name, in `app_local_data_dir()`.
const CRASH_FILE_NAME: &str = "crash.json";
/// How long after `setup` the previous run's crash is reported: long enough
/// for the dock to be on screen first.
const REPORT_DELAY: Duration = Duration::from_secs(2);
/// The dialog's title bar.
const DIALOG_TITLE: &str = "Stem";

/// Where the markers live, once [`init`] has resolved it. The panic hook
/// reads it without an `AppHandle`; unset means "write no marker".
static MARKER_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Resolve where the markers live, read what the previous run left, write
/// this run's marker and, after a crash, schedule the report. Runs in
/// `setup`, right after logging is up and **before** `panic_test`, so that
/// the test panic leaves a marker like a real one. Under the mock platform
/// nothing is read or written; without an app local data dir there is
/// nowhere to keep markers, so crashes are not reported this session and
/// the reason is logged once.
pub fn init(app: &AppHandle, platform: &dyn Platform) {
    if is_mock(platform) {
        log::info!("crash: the mock platform writes no markers");
        return;
    }
    let dir = match app.path().app_local_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!(
                "crash: no app local data dir ({e}); a crash will not be reported at the next start"
            );
            return;
        }
    };
    if MARKER_DIR.set(dir.clone()).is_err() {
        log::debug!("crash: init called twice; keeping the first marker dir");
    }

    let crash = read_marker::<CrashMarker>(&dir.join(CRASH_FILE_NAME));
    let run = read_marker::<RunMarker>(&dir.join(RUN_FILE_NAME));
    match assess(crash, run, std::process::id()) {
        Verdict::Clean => {
            log::debug!("crash: the previous run exited cleanly (or this is the first)")
        }
        Verdict::UncleanExit(run) => log::warn!(
            "the previous run (pid {}, started {}, version {}) did not exit cleanly",
            run.pid,
            run.started_at,
            run.version
        ),
        Verdict::Crashed(crash) => {
            log::warn!(
                "the previous run crashed at {} ({}): {}; reporting it in {} s",
                crash.location,
                crash.time,
                crash.message,
                REPORT_DELAY.as_secs()
            );
            let log_dir = app.path().app_log_dir().ok();
            schedule_report(app.clone(), crash, dir.clone(), log_dir);
        }
    }

    let marker = RunMarker {
        started_at: rfc3339_utc(SystemTime::now()),
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let path = dir.join(RUN_FILE_NAME);
    match write_marker(&path, &marker) {
        Ok(()) => log::debug!("crash: run marker written to {}", path.display()),
        Err(e) => log::warn!(
            "crash: could not write the run marker {} ({e}); an unclean exit will not be noticed",
            path.display()
        ),
    }
}

/// The panic hook's part: write `crash.json` for this panic, if [`init`]
/// has run. Best effort, and nothing in here panics (that would abort the
/// process inside the hook); a failure is logged and the hook goes on.
pub fn record_panic(message: &str, location: &str) {
    let Some(dir) = MARKER_DIR.get() else {
        return;
    };
    let path = dir.join(CRASH_FILE_NAME);
    if let Err(e) = write_crash_marker(&path, message, location) {
        log::warn!(
            "crash: could not write the crash marker {} ({e})",
            path.display()
        );
    }
}

/// `RunEvent::Exit`, the clean quit path: the run marker goes. Inline on
/// the exiting thread, after the taskbar restore (one small file delete).
pub fn on_exit() {
    let Some(dir) = MARKER_DIR.get() else {
        return;
    };
    let path = dir.join(RUN_FILE_NAME);
    match delete_marker(&path) {
        Ok(()) => log::debug!("crash: clean exit; run marker deleted"),
        Err(e) => log::warn!(
            "crash: could not delete the run marker {} ({e}); the next start will report an unclean exit",
            path.display()
        ),
    }
}

/// Run [`report`] on the blocking pool after [`REPORT_DELAY`]. The service
/// is looked up then, not now: `init` runs before it exists.
fn schedule_report(app: AppHandle, crash: CrashMarker, dir: PathBuf, log_dir: Option<PathBuf>) {
    drop(tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(REPORT_DELAY);
        let Some(service) = app.try_state::<AppService>() else {
            // `setup` did not finish; the app is on its way out and the
            // marker stays for the next start.
            return;
        };
        let platform = service.0.platform();
        report(platform.as_ref(), &crash, &dir, log_dir.as_deref());
    }));
}

/// Tell the user about `crash`, open the log folder on *Yes*, and delete
/// the crash marker in `dir` whatever happened. Blocking pool: the box
/// stays up until the user answers.
fn report(platform: &dyn Platform, crash: &CrashMarker, dir: &Path, log_dir: Option<&Path>) {
    match platform.ask_yes_no(DIALOG_TITLE, &dialog_text(crash)) {
        Ok(true) => {
            let folder = folder_to_open(dir, log_dir);
            let target = LaunchTarget::Uri {
                uri: folder.to_string_lossy().into_owned(),
            };
            match platform.launch(&target) {
                Ok(()) => log::info!("crash: opened the log folder {}", folder.display()),
                Err(e) => log::warn!(
                    "crash: could not open the log folder {} ({e})",
                    folder.display()
                ),
            }
        }
        Ok(false) => log::info!("crash: the user did not want the log folder"),
        Err(e) => log::warn!(
            "crash: could not show the crash dialog ({e}); the previous run's panic is in the log"
        ),
    }
    let path = dir.join(CRASH_FILE_NAME);
    match delete_marker(&path) {
        Ok(()) => log::debug!("crash: crash marker deleted; the panic was reported once"),
        Err(e) => log::warn!(
            "crash: could not delete the crash marker {} ({e}); the next start will report it again",
            path.display()
        ),
    }
}

/// The log folder when it exists (file logging has been on at some point),
/// else the markers' folder, its parent, which always exists by now.
fn folder_to_open(dir: &Path, log_dir: Option<&Path>) -> PathBuf {
    match log_dir {
        Some(logs) if logs.is_dir() => logs.to_path_buf(),
        _ => dir.to_path_buf(),
    }
}

/// Write `crash.json` at `path` for a panic seen now.
fn write_crash_marker(path: &Path, message: &str, location: &str) -> std::io::Result<()> {
    write_marker(
        path,
        &CrashMarker {
            kind: CrashKind::Panic,
            message: message.to_owned(),
            location: location.to_owned(),
            time: rfc3339_utc(SystemTime::now()),
        },
    )
}
