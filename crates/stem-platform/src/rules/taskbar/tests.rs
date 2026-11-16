use pretty_assertions::assert_eq;

use super::*;

const WAS_OFF: Marker = Marker {
    previous_auto_hide: false,
};
const WAS_ON: Marker = Marker {
    previous_auto_hide: true,
};

#[test]
fn first_start_with_the_setting_on_remembers_off_and_hides() {
    assert_eq!(
        plan(true, None, Some(false)),
        Plan {
            write_marker: Some(WAS_OFF),
            set_auto_hide: Some(true),
            delete_marker: false,
        }
    );
}

#[test]
fn setting_on_while_windows_already_auto_hides_remembers_on_and_sets_nothing() {
    assert_eq!(
        plan(true, None, Some(true)),
        Plan {
            write_marker: Some(WAS_ON),
            set_auto_hide: None,
            delete_marker: false,
        }
    );
}

#[test]
fn crash_recovery_with_the_setting_on_keeps_the_marker_and_makes_sure_it_hides() {
    // Windows kept the state across the crash: nothing to do.
    assert_eq!(plan(true, Some(WAS_OFF), Some(true)), Plan::NOTHING);
    // The user turned it back off in between: on again, same marker.
    assert_eq!(
        plan(true, Some(WAS_OFF), Some(false)),
        Plan {
            write_marker: None,
            set_auto_hide: Some(true),
            delete_marker: false,
        }
    );
    // Whatever the marker says, it is kept as it is.
    assert_eq!(
        plan(true, Some(WAS_ON), Some(false)),
        Plan {
            write_marker: None,
            set_auto_hide: Some(true),
            delete_marker: false,
        }
    );
}

#[test]
fn crash_recovery_with_the_setting_off_restores_and_deletes() {
    assert_eq!(
        plan(false, Some(WAS_OFF), Some(true)),
        Plan {
            write_marker: None,
            set_auto_hide: Some(false),
            delete_marker: true,
        }
    );
}

#[test]
fn setting_off_without_a_marker_leaves_the_taskbar_alone() {
    // Even an auto-hiding taskbar: Windows' own setting, not Stem's.
    assert_eq!(plan(false, None, Some(true)), Plan::NOTHING);
    assert_eq!(plan(false, None, Some(false)), Plan::NOTHING);
    assert!(plan(false, None, None).is_nothing());
}

#[test]
fn setting_off_when_the_original_state_is_back_only_deletes_the_marker() {
    assert_eq!(
        plan(false, Some(WAS_ON), Some(true)),
        Plan {
            write_marker: None,
            set_auto_hide: None,
            delete_marker: true,
        }
    );
    assert_eq!(
        plan(false, Some(WAS_OFF), Some(false)),
        Plan {
            write_marker: None,
            set_auto_hide: None,
            delete_marker: true,
        }
    );
}

#[test]
fn exit_with_a_marker_restores_and_deletes() {
    assert_eq!(
        plan_exit(Some(WAS_OFF), Some(true)),
        Plan {
            write_marker: None,
            set_auto_hide: Some(false),
            delete_marker: true,
        }
    );
    // The user had auto-hide before Stem: it stays, the marker goes.
    assert_eq!(
        plan_exit(Some(WAS_ON), Some(true)),
        Plan {
            write_marker: None,
            set_auto_hide: None,
            delete_marker: true,
        }
    );
}

#[test]
fn exit_without_a_marker_does_nothing() {
    assert_eq!(plan_exit(None, Some(true)), Plan::NOTHING);
    assert_eq!(plan_exit(None, Some(false)), Plan::NOTHING);
    assert_eq!(plan_exit(None, None), Plan::NOTHING);
}

#[test]
fn an_unreadable_live_state_without_a_marker_changes_nothing() {
    // No tray (Explorer crashed or restarting) or no answer: there is no
    // state to remember, so nothing is written and nothing is set.
    assert_eq!(plan(true, None, None), Plan::NOTHING);
    assert_eq!(plan(false, None, None), Plan::NOTHING);
    assert_eq!(plan_exit(None, None), Plan::NOTHING);
}

#[test]
fn an_unreadable_live_state_with_a_marker_sets_but_keeps_the_marker() {
    // Setting on: the marker is kept as it is and auto-hide is set
    // without a comparison.
    assert_eq!(
        plan(true, Some(WAS_ON), None),
        Plan {
            write_marker: None,
            set_auto_hide: Some(true),
            delete_marker: false,
        }
    );
    // Setting off and exit: the restore is attempted, the marker stays
    // for the next start to finish the job with a readable state.
    assert_eq!(
        plan(false, Some(WAS_OFF), None),
        Plan {
            write_marker: None,
            set_auto_hide: Some(false),
            delete_marker: false,
        }
    );
    assert_eq!(
        plan_exit(Some(WAS_ON), None),
        Plan {
            write_marker: None,
            set_auto_hide: Some(true),
            delete_marker: false,
        }
    );
}

#[test]
fn an_unreadable_live_state_never_writes_or_deletes_a_marker() {
    let markers = [None, Some(WAS_OFF), Some(WAS_ON)];
    for marker in markers {
        for hide in [false, true] {
            let planned = plan(hide, marker, None);
            assert_eq!(planned.write_marker, None, "plan({hide}, {marker:?}, None)");
            assert!(!planned.delete_marker, "plan({hide}, {marker:?}, None)");
        }
        let planned = plan_exit(marker, None);
        assert_eq!(planned.write_marker, None, "plan_exit({marker:?}, None)");
        assert!(!planned.delete_marker, "plan_exit({marker:?}, None)");
    }
}

#[test]
fn the_start_after_an_unreadable_exit_finishes_the_restore() {
    // Exit with Explorer gone: the restore is attempted, the marker stays.
    let exit = plan_exit(Some(WAS_OFF), None);
    assert!(!exit.delete_marker);
    // The next start, setting off, with the state readable again (the
    // set at exit may or may not have landed): restore and delete.
    assert_eq!(
        plan(false, Some(WAS_OFF), Some(true)),
        Plan {
            write_marker: None,
            set_auto_hide: Some(false),
            delete_marker: true,
        }
    );
    assert_eq!(
        plan(false, Some(WAS_OFF), Some(false)),
        Plan {
            write_marker: None,
            set_auto_hide: None,
            delete_marker: true,
        }
    );
    // Setting on: the old marker still holds the user's state.
    assert_eq!(
        plan(true, Some(WAS_OFF), Some(false)),
        Plan {
            write_marker: None,
            set_auto_hide: Some(true),
            delete_marker: false,
        }
    );
}

#[test]
fn nothing_is_the_default_plan() {
    assert_eq!(Plan::default(), Plan::NOTHING);
    assert!(Plan::NOTHING.is_nothing());
    assert!(
        !Plan {
            delete_marker: true,
            ..Plan::NOTHING
        }
        .is_nothing()
    );
}
