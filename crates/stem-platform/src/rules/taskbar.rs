//! What Stem does to the Windows taskbar's auto-hide state.
//!
//! `system.hideWindowsTaskbar` hides nothing itself: it turns on Windows'
//! own *Automatically hide the taskbar* (`SHAppBarMessage` with
//! `ABM_SETSTATE`, `ABS_AUTOHIDE`), which is reversible, which the Taskbar
//! settings page shows, and which survives an Explorer restart. Windows
//! persists it, so Stem must remember what the user had and put it back:
//! that is the **marker**, a small file the app layer keeps (`taskbar.json`,
//! `{"previousAutoHide": bool}`) that holds the state before Stem's first
//! change, written before that change and deleted after the restore. A
//! marker that is still there when Stem starts means the previous run did
//! not restore (a crash, *End task*), and the state it holds is still the
//! one to go back to.
//!
//! [`plan`] and [`plan_exit`] are the whole decision, pure so that they are
//! tested on every host. The app layer gathers the inputs (the marker, the
//! live state) and performs the [`Plan`] in its field order: write the
//! marker, set the state, delete the marker, and the delete only after a set
//! that succeeded, so that a failed set is retried at the next start.
//!
//! The rules:
//! - setting **on**: with no marker, remember the live state and turn
//!   auto-hide on (unless it already is); with a marker (the previous run
//!   did not restore), keep it and just make sure auto-hide is on;
//! - setting **off**, and the app's exit: with a marker, put back the state
//!   it holds and delete it; with none, nothing (Stem never touched the
//!   taskbar, whatever its state is);
//! - a live state that could not be read (`None`: no tray, Explorer
//!   restarting or not answering) never writes a new marker and never
//!   deletes an existing one. With no marker there is nothing to put back
//!   and no way to know what to remember, so setting on changes nothing (a
//!   state that cannot be restored is not changed) until the next sync (a
//!   later start, or the setting flipped). With a marker, the set is still
//!   attempted without a comparison (it is idempotent), but the marker
//!   stays, so the next start, with a readable state, finishes the restore.
//!   Reading `None` as "off" would delete the marker after a restore that
//!   never happened and remember the wrong state at the next start.
//!
//! Not tracked: the user flipping Windows' own setting while Stem runs. The
//! restore puts back what the marker holds.

/// What the marker remembers: the taskbar's auto-hide state before Stem's
/// first change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    pub previous_auto_hide: bool,
}

/// What the app layer must do, in field order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Plan {
    /// First, write this marker (there is none yet). When the write fails,
    /// nothing else happens: a state that cannot be restored is not changed.
    pub write_marker: Option<Marker>,
    /// Then set the taskbar's auto-hide to this.
    pub set_auto_hide: Option<bool>,
    /// Then delete the marker, provided the set (when there was one)
    /// succeeded; otherwise it stays and the next start retries.
    pub delete_marker: bool,
}

impl Plan {
    /// Leave everything as it is.
    pub const NOTHING: Plan = Plan {
        write_marker: None,
        set_auto_hide: None,
        delete_marker: false,
    };

    pub fn is_nothing(&self) -> bool {
        *self == Self::NOTHING
    }
}

/// The plan for the app's start, or for a settings change, with
/// `system.hideWindowsTaskbar` = `hide`. `marker` is the marker as it is
/// (`None`: no file); `live` is the taskbar's auto-hide state right now
/// (`None`: it could not be read).
pub fn plan(hide: bool, marker: Option<Marker>, live: Option<bool>) -> Plan {
    if !hide {
        return restore(marker, live);
    }
    match (marker, live) {
        // The previous run did not restore: keep its marker.
        (Some(_), live) => Plan {
            write_marker: None,
            set_auto_hide: set_unless(live, true),
            delete_marker: false,
        },
        (None, Some(live)) => Plan {
            write_marker: Some(Marker {
                previous_auto_hide: live,
            }),
            set_auto_hide: set_unless(Some(live), true),
            delete_marker: false,
        },
        // Nothing to remember it by: leave the taskbar alone.
        (None, None) => Plan::NOTHING,
    }
}

/// The plan for the app's exit: put back what the marker holds, if any.
/// The setting plays no part; the same as [`plan`] with `hide` off.
pub fn plan_exit(marker: Option<Marker>, live: Option<bool>) -> Plan {
    restore(marker, live)
}

/// Put back what `marker` holds; delete it only when the live state could
/// be read (otherwise the next start finishes the restore).
fn restore(marker: Option<Marker>, live: Option<bool>) -> Plan {
    match marker {
        Some(marker) => Plan {
            write_marker: None,
            set_auto_hide: set_unless(live, marker.previous_auto_hide),
            delete_marker: live.is_some(),
        },
        None => Plan::NOTHING,
    }
}

/// `Some(wanted)` unless the live state is known to be `wanted` already.
fn set_unless(live: Option<bool>, wanted: bool) -> Option<bool> {
    (live != Some(wanted)).then_some(wanted)
}

#[cfg(test)]
mod tests {
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
}
