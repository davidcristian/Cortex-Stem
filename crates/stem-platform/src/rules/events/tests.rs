use pretty_assertions::assert_eq;

use super::*;

#[test]
fn event_table_matches_the_contract() {
    assert_eq!(event_intent(EVENT_OBJECT_CREATE), Intent::Added);
    assert_eq!(event_intent(EVENT_OBJECT_SHOW), Intent::Added);
    assert_eq!(event_intent(EVENT_OBJECT_DESTROY), Intent::Removed);
    assert_eq!(event_intent(EVENT_OBJECT_HIDE), Intent::Removed);
    assert_eq!(event_intent(EVENT_OBJECT_NAMECHANGE), Intent::TitleChanged);
    assert_eq!(event_intent(EVENT_SYSTEM_MINIMIZESTART), Intent::Updated);
    assert_eq!(event_intent(EVENT_SYSTEM_MINIMIZEEND), Intent::Updated);
    assert_eq!(event_intent(EVENT_OBJECT_CLOAKED), Intent::Updated);
    assert_eq!(event_intent(EVENT_OBJECT_UNCLOAKED), Intent::Updated);
    assert_eq!(event_intent(EVENT_SYSTEM_FOREGROUND), Intent::Foreground);
    // EVENT_OBJECT_LOCATIONCHANGE has its own hook and callback;
    // in the table it, like any stray id, must be harmless.
    assert_eq!(EVENT_OBJECT_LOCATIONCHANGE, 0x800B);
    assert_eq!(event_intent(EVENT_OBJECT_LOCATIONCHANGE), Intent::Ignore);
    assert_eq!(event_intent(0), Intent::Ignore);
}

#[test]
fn hook_ranges_cover_exactly_the_table() {
    let mut hooked = Vec::new();
    for &(min, max) in HOOK_RANGES {
        assert!(min <= max);
        for event in min..=max {
            assert_ne!(
                event_intent(event),
                Intent::Ignore,
                "hooked event {event:#06x} has no intent"
            );
            hooked.push(event);
        }
    }
    for &event in &hooked {
        assert!(event_name(event).starts_with("EVENT_"), "{event:#06x}");
    }
    assert_eq!(event_name(EVENT_OBJECT_LOCATIONCHANGE), "an unhooked event");
    assert!(!hooked.contains(&EVENT_OBJECT_LOCATIONCHANGE));
    hooked.sort_unstable();
    hooked.dedup();
    assert_eq!(
        hooked,
        vec![
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MINIMIZEEND,
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_DESTROY,
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_HIDE,
            EVENT_OBJECT_NAMECHANGE,
            EVENT_OBJECT_CLOAKED,
            EVENT_OBJECT_UNCLOAKED,
        ]
    );
}

#[test]
fn callback_filter_rejects_children_and_sub_objects() {
    let ev = EVENT_OBJECT_SHOW;
    assert_eq!(
        win_event_intent(ev, OBJID_WINDOW, CHILDID_SELF, TopLevel::Yes),
        Intent::Added
    );
    // OBJID_CLIENT (-4), OBJID_CARET (-8), ... are not the window.
    assert_eq!(
        win_event_intent(ev, -4, CHILDID_SELF, TopLevel::Yes),
        Intent::Ignore
    );
    // A child id means a list item / menu entry, not the window.
    assert_eq!(
        win_event_intent(ev, OBJID_WINDOW, 3, TopLevel::Yes),
        Intent::Ignore
    );
    // Child windows (controls) are not top-level.
    assert_eq!(
        win_event_intent(ev, OBJID_WINDOW, CHILDID_SELF, TopLevel::No),
        Intent::Ignore
    );
}

#[test]
fn destroyed_windows_are_forgotten_even_when_already_gone() {
    assert_eq!(
        win_event_intent(
            EVENT_OBJECT_DESTROY,
            OBJID_WINDOW,
            CHILDID_SELF,
            TopLevel::Unknown
        ),
        Intent::Removed
    );
    assert_eq!(
        win_event_intent(
            EVENT_OBJECT_HIDE,
            OBJID_WINDOW,
            CHILDID_SELF,
            TopLevel::Unknown
        ),
        Intent::Removed
    );
    // ...but nothing else is accepted for a window we cannot inspect.
    for ev in [
        EVENT_OBJECT_SHOW,
        EVENT_OBJECT_NAMECHANGE,
        EVENT_SYSTEM_FOREGROUND,
        EVENT_SYSTEM_MINIMIZESTART,
    ] {
        assert_eq!(
            win_event_intent(ev, OBJID_WINDOW, CHILDID_SELF, TopLevel::Unknown),
            Intent::Ignore,
            "{ev:#06x}"
        );
    }
}

#[test]
fn location_changes_count_only_for_whole_top_level_windows() {
    assert!(location_change_tracked(
        OBJID_WINDOW,
        CHILDID_SELF,
        TopLevel::Yes
    ));
    // The cursor (-9) and the caret (-8) move with every mouse move and
    // keystroke; the client area (-4) is not the window.
    for object in [-9, -8, -4] {
        assert!(!location_change_tracked(
            object,
            CHILDID_SELF,
            TopLevel::Yes
        ));
    }
    assert!(!location_change_tracked(OBJID_WINDOW, 1, TopLevel::Yes));
    assert!(!location_change_tracked(
        OBJID_WINDOW,
        CHILDID_SELF,
        TopLevel::No
    ));
    // Nothing can be queried about a window that is already gone.
    assert!(!location_change_tracked(
        OBJID_WINDOW,
        CHILDID_SELF,
        TopLevel::Unknown
    ));
}

#[test]
fn shell_hook_codes() {
    assert_eq!(HSHELL_RUDEAPPACTIVATED, 0x8004);
    assert_eq!(HSHELL_FLASH, 0x8006);
    assert_eq!(
        shell_hook_intent(HSHELL_WINDOWACTIVATED),
        Intent::Foreground
    );
    assert_eq!(
        shell_hook_intent(HSHELL_RUDEAPPACTIVATED),
        Intent::Foreground
    );
    assert_eq!(shell_hook_intent(HSHELL_FLASH), Intent::Attention);
    // Plain HSHELL_REDRAW (title/icon repaint) is not a flash.
    assert_eq!(shell_hook_intent(HSHELL_REDRAW), Intent::Ignore);
    // A new taskbar window is a show-type event.
    assert_eq!(HSHELL_WINDOWCREATED, 1);
    assert_eq!(shell_hook_intent(HSHELL_WINDOWCREATED), Intent::Added);
    // HSHELL_WINDOWDESTROYED is covered by EVENT_OBJECT_DESTROY.
    assert_eq!(HSHELL_WINDOWDESTROYED, 2);
    assert_eq!(shell_hook_intent(HSHELL_WINDOWDESTROYED), Intent::Ignore);
    assert_eq!(shell_hook_intent(0), Intent::Ignore);
}

/// Every code `winuser.h` documents has a name (so the per-message trace
/// line can be counted per source), and only the four translated
/// ones have an intent other than `Ignore`.
#[test]
fn every_shell_hook_code_is_named_and_only_four_are_translated() {
    let documented = [
        (HSHELL_WINDOWCREATED, "HSHELL_WINDOWCREATED", Intent::Added),
        (
            HSHELL_WINDOWDESTROYED,
            "HSHELL_WINDOWDESTROYED",
            Intent::Ignore,
        ),
        (
            HSHELL_ACTIVATESHELLWINDOW,
            "HSHELL_ACTIVATESHELLWINDOW",
            Intent::Ignore,
        ),
        (
            HSHELL_WINDOWACTIVATED,
            "HSHELL_WINDOWACTIVATED",
            Intent::Foreground,
        ),
        (HSHELL_GETMINRECT, "HSHELL_GETMINRECT", Intent::Ignore),
        (HSHELL_REDRAW, "HSHELL_REDRAW", Intent::Ignore),
        (HSHELL_TASKMAN, "HSHELL_TASKMAN", Intent::Ignore),
        (HSHELL_LANGUAGE, "HSHELL_LANGUAGE", Intent::Ignore),
        (HSHELL_SYSMENU, "HSHELL_SYSMENU", Intent::Ignore),
        (HSHELL_ENDTASK, "HSHELL_ENDTASK", Intent::Ignore),
        (
            HSHELL_ACCESSIBILITYSTATE,
            "HSHELL_ACCESSIBILITYSTATE",
            Intent::Ignore,
        ),
        (HSHELL_APPCOMMAND, "HSHELL_APPCOMMAND", Intent::Ignore),
        (
            HSHELL_WINDOWREPLACED,
            "HSHELL_WINDOWREPLACED",
            Intent::Ignore,
        ),
        (
            HSHELL_WINDOWREPLACING,
            "HSHELL_WINDOWREPLACING",
            Intent::Ignore,
        ),
        (
            HSHELL_MONITORCHANGED,
            "HSHELL_MONITORCHANGED",
            Intent::Ignore,
        ),
        (
            HSHELL_RUDEAPPACTIVATED,
            "HSHELL_RUDEAPPACTIVATED",
            Intent::Foreground,
        ),
        (HSHELL_FLASH, "HSHELL_FLASH", Intent::Attention),
    ];
    // winuser.h numbers them 1..=14 and 16, plus the two high-bit forms.
    let mut codes: Vec<u32> = documented.iter().map(|&(code, _, _)| code).collect();
    codes.sort_unstable();
    let mut expected: Vec<u32> = (1..=14).collect();
    expected.extend([16, 0x8004, 0x8006]);
    assert_eq!(codes, expected);
    for (code, name, intent) in documented {
        assert_eq!(shell_hook_name(code), name, "{code:#x}");
        assert_eq!(shell_hook_intent(code), intent, "{name}");
    }
    for stray in [0, 15, 17, 0x8000, 0x8001, 0x8005, u32::MAX] {
        assert_eq!(shell_hook_name(stray), "an unknown shell hook code");
        assert_eq!(shell_hook_intent(stray), Intent::Ignore);
    }
}

#[test]
fn debouncer_coalesces_per_window_and_flushes_once() {
    let mut d = TitleDebouncer::default();
    assert!(d.is_empty());
    assert_eq!(d.next_due(), None);

    assert!(d.record(1, 100));
    assert_eq!(d.next_due(), Some(150));
    // Storm inside the window: absorbed.
    assert!(!d.record(1, 110));
    assert!(!d.record(1, 149));
    assert_eq!(d.next_due(), Some(150));
    // Another window has its own timer.
    assert!(d.record(2, 120));
    assert_eq!(d.next_due(), Some(150));

    assert_eq!(d.take_due(149), Vec::<u64>::new());
    assert_eq!(d.take_due(150), vec![1]);
    assert_eq!(d.next_due(), Some(170));
    assert_eq!(d.take_due(200), vec![2]);
    assert!(d.is_empty());

    // After a flush the next change starts a new window.
    assert!(d.record(1, 200));
    assert_eq!(d.next_due(), Some(250));
}

#[test]
fn debouncer_bounds_the_rate_for_a_continuously_changing_title() {
    let mut d = TitleDebouncer::default();
    let mut flushes = 0;
    let mut now = 0;
    while now < 1000 {
        d.record(7, now);
        if !d.take_due(now).is_empty() {
            flushes += 1;
        }
        now += 5;
    }
    // A flush at t=50 closes the window; the next change (t=55) opens a
    // new one, so flushes happen every 55 ms: at 50, 105, ..., 985.
    assert_eq!(flushes, 18);
    assert!(flushes <= 1000 / TitleDebouncer::WINDOW_MS);
}

#[test]
fn forget_drops_a_pending_change() {
    let mut d = TitleDebouncer::default();
    d.record(5, 0);
    d.forget(5);
    assert!(d.is_empty());
    assert_eq!(d.take_due(1000), Vec::<u64>::new());
    // Forgetting an unknown id is a no-op.
    d.forget(6);
}

#[test]
fn two_cloak_events_within_100ms_arm_one_snapshot_until_reset() {
    let mut d = DesktopSwitchDetector::default();
    assert!(!d.is_armed());
    // Win+Ctrl+Right: the shell cloaks the windows of one desktop and
    // uncloaks those of the other, a few ms apart.
    assert!(!d.observe(1_000));
    assert!(d.observe(1_003));
    assert!(d.is_armed());
    // The rest of the burst, and anything until the snapshot, changes nothing.
    assert!(!d.observe(1_005));
    assert!(!d.observe(1_200));
    assert!(!d.observe(1_399));
    assert!(d.is_armed());
    // The snapshot at ~1 400 ms resets; the next switch arms again.
    d.reset();
    assert!(!d.is_armed());
    assert!(!d.observe(5_000));
    assert!(d.observe(5_010));
    assert_eq!(DesktopSwitchDetector::SNAPSHOT_DELAY_MS, 400);
}

#[test]
fn a_lone_cloak_event_arms_nothing_and_the_window_is_measured_from_the_first() {
    let mut d = DesktopSwitchDetector::default();
    // A window moved to another desktop, a Store app suspending, the
    // Start menu opening and closing: one event each, seconds apart.
    for now in [0, 2_000, 9_000, 20_000] {
        assert!(!d.observe(now), "{now}");
        assert!(!d.is_armed());
    }
    // Exactly the window counts; one more ms starts a new burst.
    let mut d = DesktopSwitchDetector::default();
    assert!(!d.observe(100));
    assert!(d.observe(200));
    let mut d = DesktopSwitchDetector::default();
    assert!(!d.observe(100));
    assert!(!d.observe(201));
    // ...which the next event completes.
    assert!(d.observe(250));
    // Three events spread 60 ms apart: the first two are more than the
    // window apart from the third only if measured from the first.
    let mut d = DesktopSwitchDetector::default();
    assert!(!d.observe(0));
    assert!(d.observe(60));
}

#[test]
fn desktop_query_failures_are_logged_once_per_window_and_bounded() {
    let mut failures = DesktopQueryFailures::new();
    assert!(failures.is_empty());
    assert!(failures.note(0x1_0203));
    assert!(!failures.note(0x1_0203));
    assert!(failures.note(0x1_0204));
    assert_eq!(failures.len(), 2);
    // A later success forgets the refusal: the handle, reused by another
    // window that refuses, is logged again.
    failures.clear(0x1_0203);
    assert!(failures.note(0x1_0203));
    failures.clear(99);
    assert_eq!(failures.len(), 2);
    // Full, the set starts over rather than growing.
    let mut failures = DesktopQueryFailures::new();
    for id in 0..DesktopQueryFailures::CAPACITY as u64 {
        assert!(failures.note(id), "{id}");
    }
    assert!(!failures.note(0));
    assert!(failures.note(DesktopQueryFailures::CAPACITY as u64));
    assert_eq!(failures.len(), 1);
    assert!(failures.note(0));
}
