use std::sync::Arc;

use parking_lot::Mutex;
use stem_core::{PlatformEvent, ShellAction, WindowId};

use crate::Platform;
use crate::mock::demo::demo_power;
use crate::mock::{MockCall, MockPlatform};

#[test]
fn the_date_time_and_power_settings_pages_are_recorded_and_change_nothing() {
    let p = MockPlatform::with_demo_scenario();
    let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
    let sink_seen = Arc::clone(&seen);
    p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
        .unwrap();
    let before = seen.lock().len();
    p.shell_action(ShellAction::OpenDateTimeSettings).unwrap();
    p.shell_action(ShellAction::OpenPowerSettings).unwrap();
    assert_eq!(
        p.calls(),
        vec![
            MockCall::Shell(ShellAction::OpenDateTimeSettings),
            MockCall::Shell(ShellAction::OpenPowerSettings),
        ]
    );
    assert_eq!(seen.lock().len(), before);
    assert_eq!(p.power_status().unwrap(), demo_power());
}

#[test]
fn quick_settings_and_notification_center_are_recorded_and_change_nothing() {
    let p = MockPlatform::with_demo_scenario();
    let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
    let sink_seen = Arc::clone(&seen);
    p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
        .unwrap();
    let before = seen.lock().len();
    let windows = p.windows().unwrap();

    p.shell_action(ShellAction::OpenQuickSettings).unwrap();
    p.shell_action(ShellAction::OpenNotificationCenter).unwrap();
    assert_eq!(
        p.calls(),
        vec![
            MockCall::Shell(ShellAction::OpenQuickSettings),
            MockCall::Shell(ShellAction::OpenNotificationCenter),
        ]
    );
    // Windows' flyouts are not windows the dock tracks: no event, the
    // same windows, the same foreground.
    assert_eq!(seen.lock().len(), before);
    assert_eq!(p.windows().unwrap(), windows);
    assert_eq!(p.foreground_window().unwrap(), Some(WindowId(2)));
}

#[test]
fn delete_to_recycle_bin_records_the_paths_and_fills_the_bin_silently() {
    let p = MockPlatform::new();
    let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
    let sink_seen = Arc::clone(&seen);
    p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
        .unwrap();
    let before = seen.lock().len();
    assert!(p.recycle_bin().unwrap().is_empty());
    let paths = vec![
        r"C:\Users\me\Desktop\notes.txt".to_owned(),
        r"C:\Users\me\Desktop\Folder".to_owned(),
        "relative.txt".to_owned(),
    ];

    p.delete_to_recycle_bin(&paths).unwrap();
    assert_eq!(p.calls(), vec![MockCall::DeleteToRecycleBin(paths)]);
    // The relative path is skipped; the other two are in the bin.
    assert_eq!(p.recycle_bin().unwrap().item_count, 2);
    assert_eq!(seen.lock().len(), before, "no event: the caller asks");

    // Nothing usable: an error, the bin unchanged.
    let err = p.delete_to_recycle_bin(&[r"C:\".to_owned()]).unwrap_err();
    assert_eq!(err.to_string(), r"C:\ is a drive or share root");
    assert_eq!(p.recycle_bin().unwrap().item_count, 2);

    // A scripted refusal: recorded, an error, the bin unchanged.
    p.set_recycle_failure(Some("notes.txt is open in another program"));
    let locked = vec![r"C:\Users\me\locked.docx".to_owned()];
    let err = p.delete_to_recycle_bin(&locked).unwrap_err();
    assert_eq!(err.to_string(), "notes.txt is open in another program");
    assert_eq!(
        p.calls().last(),
        Some(&MockCall::DeleteToRecycleBin(locked.clone()))
    );
    assert_eq!(p.recycle_bin().unwrap().item_count, 2);
    p.set_recycle_failure(None);
    p.delete_to_recycle_bin(&locked).unwrap();
    assert_eq!(p.recycle_bin().unwrap().item_count, 3);
}

#[test]
fn taskbar_auto_hide_is_scripted_follows_sets_and_records_every_call() {
    let p = MockPlatform::new();
    assert!(!p.taskbar_auto_hide().unwrap(), "off by default");
    p.set_taskbar_auto_hide(true).unwrap();
    assert!(p.taskbar_auto_hide().unwrap());
    p.set_taskbar_auto_hide(false).unwrap();
    assert!(!p.taskbar_auto_hide().unwrap());
    assert_eq!(
        p.calls(),
        vec![
            MockCall::TaskbarAutoHide,
            MockCall::SetTaskbarAutoHide(true),
            MockCall::TaskbarAutoHide,
            MockCall::SetTaskbarAutoHide(false),
            MockCall::TaskbarAutoHide,
        ]
    );

    // What the user had before Stem: scripted, not a call.
    p.clear_calls();
    p.script_taskbar_auto_hide(true);
    assert!(p.taskbar_auto_hide().unwrap());
    assert_eq!(p.calls(), vec![MockCall::TaskbarAutoHide]);
}

#[test]
fn ask_yes_no_answers_the_scripted_value_and_records_the_question() {
    let p = MockPlatform::new();
    assert!(
        !p.ask_yes_no("Stem", "Open the log folder?").unwrap(),
        "No by default"
    );
    p.script_yes_no_answer(true);
    assert!(p.ask_yes_no("Stem", "Again?").unwrap());
    assert_eq!(
        p.calls(),
        vec![
            MockCall::AskYesNo {
                title: "Stem".to_owned(),
                text: "Open the log folder?".to_owned(),
            },
            MockCall::AskYesNo {
                title: "Stem".to_owned(),
                text: "Again?".to_owned(),
            },
        ]
    );
}
