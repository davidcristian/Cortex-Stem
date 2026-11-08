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
//! `taskbar.rs` (atomic temp file + rename, an unreadable file counts as
//! absent after a warning), copied rather than shared: this module has no
//! in-memory variant, because under the mock platform (demo runs) nothing
//! is written and the check does nothing, like autostart and taskbar. No
//! external crate for the time stamp: [`rfc3339_utc`] formats a
//! `SystemTime` by hand.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use stem_core::LaunchTarget;
use stem_platform::{Platform, is_mock};
use tauri::{AppHandle, Manager};

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
/// The dialog shows at most this much of the panic message; the rest is in
/// the log and in the marker.
const DIALOG_MESSAGE_CHARS: usize = 400;

/// Where the markers live, once [`init`] has resolved it. The panic hook
/// reads it without an `AppHandle`; unset means "write no marker".
static MARKER_DIR: OnceLock<PathBuf> = OnceLock::new();

/// `run.json`: the run that is (or was) in progress.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMarker {
    /// RFC 3339, UTC.
    pub started_at: String,
    pub pid: u32,
    /// The package version of the binary that wrote it.
    pub version: String,
}

/// `crash.json`: what the panic hook saw.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashMarker {
    pub kind: CrashKind,
    pub message: String,
    /// `file:line:column`, as the panic hook formats it.
    pub location: String,
    /// RFC 3339, UTC.
    pub time: String,
}

/// What kind of crash a marker records. Only panics are caught today; the
/// field exists so that another kind can be told apart later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CrashKind {
    Panic,
}

/// What the previous run left behind means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to report: a clean exit, or the first start.
    Clean,
    /// The previous run never reached `RunEvent::Exit` and did not panic:
    /// log once.
    UncleanExit(RunMarker),
    /// The previous run panicked: tell the user once.
    Crashed(CrashMarker),
}

/// The decision, pure: a crash marker wins; a run marker from another
/// process is an unclean exit; a run marker with this process's pid cannot
/// be a previous run's (this run has not written one yet) and is ignored.
pub fn assess(crash: Option<CrashMarker>, run: Option<RunMarker>, this_pid: u32) -> Verdict {
    if let Some(crash) = crash {
        return Verdict::Crashed(crash);
    }
    match run {
        Some(run) if run.pid != this_pid => Verdict::UncleanExit(run),
        _ => Verdict::Clean,
    }
}

/// The dialog's text for a crash: when, where, what, and the question. The
/// message is cut at [`DIALOG_MESSAGE_CHARS`] characters (on a character
/// boundary) with an ellipsis, so a long panic message cannot produce a
/// screen-high box.
pub fn dialog_text(crash: &CrashMarker) -> String {
    let message = match crash.message.char_indices().nth(DIALOG_MESSAGE_CHARS) {
        Some((cut, _)) => format!("{}…", &crash.message[..cut]),
        None => crash.message.clone(),
    };
    format!(
        "Stem crashed last time ({}, {}):\n{message}\n\nOpen the log folder?",
        crash.time, crash.location
    )
}

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

// ---------------------------------------------------------------------------
// The marker store (the shape of `taskbar.rs`)
// ---------------------------------------------------------------------------

/// The marker as it is; a file that cannot be read or parsed counts as
/// absent, after a warning (the next write replaces it).
fn read_marker<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
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
fn write_marker<T: serde::Serialize>(path: &Path, marker: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(marker)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Idempotent: a marker that is already gone is fine.
fn delete_marker(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

// ---------------------------------------------------------------------------
// Time stamps
// ---------------------------------------------------------------------------

/// `time` as `YYYY-MM-DDTHH:MM:SSZ` (RFC 3339, UTC, whole seconds), the way
/// the log's own time stamps read. A time before 1970 reads as the epoch.
/// Cannot panic: it runs inside the panic hook.
pub fn rfc3339_utc(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let (year, month, day) = civil_from_days(secs / 86_400);
    let of_day = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3_600,
        of_day % 3_600 / 60,
        of_day % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day).
/// Howard Hinnant's `civil_from_days`, for non-negative days only.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097; // day of era, [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year from March, [0, 365]
    let mp = (5 * doy + 2) / 153; // month from March, [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use stem_platform::mock::{MockCall, MockPlatform};

    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stem-crash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn a_run(pid: u32) -> RunMarker {
        RunMarker {
            started_at: "2026-11-08T09:00:00Z".to_owned(),
            pid,
            version: "0.1.0".to_owned(),
        }
    }

    fn a_crash() -> CrashMarker {
        CrashMarker {
            kind: CrashKind::Panic,
            message: "STEM_PANIC_TEST=1: intentional panic to exercise the panic hook".to_owned(),
            location: r"src-tauri\src\lib.rs:320:9".to_owned(),
            time: "2026-11-08T09:00:02Z".to_owned(),
        }
    }

    #[test]
    fn nothing_left_behind_is_clean() {
        assert_eq!(assess(None, None, 1234), Verdict::Clean);
    }

    #[test]
    fn a_run_marker_of_another_process_is_an_unclean_exit() {
        assert_eq!(
            assess(None, Some(a_run(4321)), 1234),
            Verdict::UncleanExit(a_run(4321))
        );
    }

    #[test]
    fn a_run_marker_with_this_pid_is_not_a_previous_run() {
        assert_eq!(assess(None, Some(a_run(1234)), 1234), Verdict::Clean);
    }

    #[test]
    fn a_crash_marker_outranks_the_run_marker() {
        assert_eq!(
            assess(Some(a_crash()), Some(a_run(4321)), 1234),
            Verdict::Crashed(a_crash())
        );
        assert_eq!(
            assess(Some(a_crash()), None, 1234),
            Verdict::Crashed(a_crash())
        );
    }

    #[test]
    fn the_dialog_says_when_where_what_and_asks() {
        assert_eq!(
            dialog_text(&a_crash()),
            "Stem crashed last time (2026-11-08T09:00:02Z, src-tauri\\src\\lib.rs:320:9):\n\
             STEM_PANIC_TEST=1: intentional panic to exercise the panic hook\n\
             \n\
             Open the log folder?"
        );
    }

    #[test]
    fn the_dialog_cuts_a_long_message_on_a_character_boundary() {
        let mut crash = a_crash();
        crash.message = "é".repeat(DIALOG_MESSAGE_CHARS + 50);
        let text = dialog_text(&crash);
        let shown = text.lines().nth(1).expect("the message is the second line");
        assert_eq!(shown.chars().count(), DIALOG_MESSAGE_CHARS + 1);
        assert!(shown.ends_with('…'));
        assert!(text.ends_with("\n\nOpen the log folder?"));

        crash.message = "é".repeat(DIALOG_MESSAGE_CHARS);
        assert!(
            !dialog_text(&crash).contains('…'),
            "exactly the limit is not cut"
        );
    }

    #[test]
    fn the_markers_round_trip_in_the_documented_shapes() {
        let dir = scratch_dir("round-trip");
        let run_path = dir.join("nested").join(RUN_FILE_NAME);
        let crash_path = dir.join("nested").join(CRASH_FILE_NAME);
        assert_eq!(read_marker::<RunMarker>(&run_path), None);
        assert_eq!(read_marker::<CrashMarker>(&crash_path), None);

        write_marker(&run_path, &a_run(1234)).unwrap();
        write_crash_marker(&crash_path, "boom", "src/x.rs:1:2").unwrap();
        assert!(!dir.join("nested").join("run.json.tmp").exists());
        assert!(!dir.join("nested").join("crash.json.tmp").exists());

        let run: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&run_path).unwrap()).unwrap();
        assert_eq!(
            run,
            serde_json::json!({
                "startedAt": "2026-11-08T09:00:00Z",
                "pid": 1234,
                "version": "0.1.0",
            })
        );
        let crash: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&crash_path).unwrap()).unwrap();
        assert_eq!(crash["kind"], "panic");
        assert_eq!(crash["message"], "boom");
        assert_eq!(crash["location"], "src/x.rs:1:2");
        let time = crash["time"].as_str().unwrap();
        assert_eq!(time.len(), "2026-11-08T09:00:02Z".len(), "{time}");
        assert!(time.ends_with('Z'));

        assert_eq!(read_marker::<RunMarker>(&run_path), Some(a_run(1234)));
        let back = read_marker::<CrashMarker>(&crash_path).unwrap();
        assert_eq!(back.kind, CrashKind::Panic);
        assert_eq!(back.message, "boom");

        delete_marker(&run_path).unwrap();
        delete_marker(&run_path).unwrap();
        assert_eq!(read_marker::<RunMarker>(&run_path), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_invalid_marker_file_counts_as_absent() {
        let dir = scratch_dir("invalid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CRASH_FILE_NAME);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(read_marker::<CrashMarker>(&path), None);
        std::fs::write(
            &path,
            r#"{"kind": "meteor", "message": "", "location": "", "time": ""}"#,
        )
        .unwrap();
        assert_eq!(read_marker::<CrashMarker>(&path), None, "unknown kind");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_report_asks_once_opens_the_folder_on_yes_and_deletes_the_marker() {
        let dir = scratch_dir("report-yes");
        let logs = dir.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let crash_path = dir.join(CRASH_FILE_NAME);
        write_marker(&crash_path, &a_crash()).unwrap();

        let platform = MockPlatform::new();
        platform.script_yes_no_answer(true);
        report(&platform, &a_crash(), &dir, Some(&logs));
        assert_eq!(
            platform.calls(),
            vec![
                MockCall::AskYesNo {
                    title: "Stem".to_owned(),
                    text: dialog_text(&a_crash()),
                },
                MockCall::Launch(LaunchTarget::Uri {
                    uri: logs.to_string_lossy().into_owned(),
                }),
            ]
        );
        assert!(!crash_path.exists(), "one dialog per crash");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_report_opens_nothing_on_no_and_still_deletes_the_marker() {
        let dir = scratch_dir("report-no");
        let crash_path = dir.join(CRASH_FILE_NAME);
        write_marker(&crash_path, &a_crash()).unwrap();

        let platform = MockPlatform::new();
        report(&platform, &a_crash(), &dir, Some(&dir.join("logs")));
        assert_eq!(platform.calls().len(), 1, "only the question");
        assert!(!crash_path.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_folder_to_open_falls_back_to_the_markers_folder() {
        let dir = scratch_dir("folder");
        std::fs::create_dir_all(&dir).unwrap();
        let logs = dir.join("logs");
        assert_eq!(folder_to_open(&dir, Some(&logs)), dir, "no logs folder yet");
        assert_eq!(folder_to_open(&dir, None), dir);
        std::fs::create_dir_all(&logs).unwrap();
        assert_eq!(folder_to_open(&dir, Some(&logs)), logs);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rfc3339_utc_matches_known_instants() {
        let at = |secs: u64| rfc3339_utc(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(at(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(at(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(at(1_735_689_599), "2024-12-31T23:59:59Z");
        assert_eq!(at(1_735_689_600), "2025-01-01T00:00:00Z");
        assert_eq!(
            rfc3339_utc(SystemTime::UNIX_EPOCH - Duration::from_secs(5)),
            "1970-01-01T00:00:00Z",
            "before the epoch reads as the epoch rather than panicking"
        );
    }

    #[test]
    fn record_panic_writes_nothing_before_init() {
        // The global is unset in this test binary (no `init` runs here), so
        // the hook's path must be a quiet no-op.
        record_panic("boom", "src/x.rs:1:2");
        assert!(MARKER_DIR.get().is_none());
    }
}
