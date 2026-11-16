use stem_platform::mock::{MockCall, MockPlatform};

use super::markers::DIALOG_MESSAGE_CHARS;
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
