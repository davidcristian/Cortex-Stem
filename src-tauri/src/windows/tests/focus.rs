use super::*;
use crate::windows::focus_release::give_focus_back;
use crate::windows::focus_session::{FocusSession, Release, ReturnTo, end_outcome};
use crate::windows::focus_shortcut::activated;
use crate::windows::focus_target::{
    focus_target, monitor_with_bounds, return_target, window_monitor,
};

fn window_on(id: u64, monitor: Option<&str>) -> stem_core::WindowInfo {
    stem_core::WindowInfo {
        id: WindowId(id),
        title: format!("window {id}"),
        is_minimized: false,
        is_active: false,
        monitor: monitor.map(|m| MonitorId(m.into())),
        icon_key: None,
        needs_attention: false,
    }
}

fn app_item(key: &str, windows: Vec<stem_core::WindowInfo>) -> stem_core::DockItem {
    stem_core::DockItem {
        key: key.into(),
        kind: stem_core::DockItemKind::App,
        app_id: None,
        title: key.into(),
        icon_key: None,
        pinned: false,
        windows,
        is_active: false,
        needs_attention: false,
        badge: None,
    }
}

#[test]
fn the_window_in_front_is_found_on_its_monitor_in_whichever_item_shows_it() {
    let state = DockState {
        items: vec![
            app_item("app:a", vec![window_on(1, Some(r"\\.\DISPLAY1"))]),
            app_item(
                "app:b",
                vec![
                    window_on(2, Some(r"\\.\DISPLAY1")),
                    window_on(3, Some(r"\\.\DISPLAY3")),
                ],
            ),
            app_item("app:c", vec![window_on(4, None)]),
        ],
        foreground: Some(WindowId(3)),
        ..DockState::default()
    };
    assert_eq!(
        window_monitor(&state, WindowId(3)),
        Some(MonitorId(r"\\.\DISPLAY3".into()))
    );
    assert_eq!(
        window_monitor(&state, WindowId(1)),
        Some(MonitorId(r"\\.\DISPLAY1".into()))
    );
    // No monitor known, or no item shows the window.
    assert_eq!(window_monitor(&state, WindowId(4)), None);
    assert_eq!(window_monitor(&state, WindowId(9)), None);
}

#[test]
fn the_focus_goes_to_the_dock_on_the_monitor_of_the_window_in_front() {
    let primary = MonitorId(r"\\.\DISPLAY1".into());
    let other = MonitorId(r"\\.\DISPLAY3".into());
    let unplugged = MonitorId(r"\\.\DISPLAY9".into());
    let docks = vec![
        (DOCK_LABEL.to_owned(), Some(primary.clone())),
        ("dock-DISPLAY3".to_owned(), Some(other.clone())),
    ];
    assert_eq!(
        focus_target(MonitorMode::All, Some(&other), &docks),
        "dock-DISPLAY3"
    );
    assert_eq!(
        focus_target(MonitorMode::All, Some(&primary), &docks),
        DOCK_LABEL
    );
    // A monitor without a dock, or no monitor known: the primary's.
    assert_eq!(
        focus_target(MonitorMode::All, Some(&unplugged), &docks),
        DOCK_LABEL
    );
    assert_eq!(focus_target(MonitorMode::All, None, &docks), DOCK_LABEL);
    // With the dock on the primary monitor only, always the primary's,
    // even while a closing secondary window is still listed.
    assert_eq!(
        focus_target(MonitorMode::Primary, Some(&other), &docks),
        DOCK_LABEL
    );
    // No dock window known at all (the set is being rebuilt).
    assert_eq!(
        focus_target(MonitorMode::All, Some(&other), &[]),
        DOCK_LABEL
    );
    // A secondary whose monitor is not known yet is never picked by it.
    let unplaced = vec![
        (DOCK_LABEL.to_owned(), None),
        ("dock-DISPLAY3".to_owned(), None),
    ];
    assert_eq!(
        focus_target(MonitorMode::All, Some(&other), &unplaced),
        DOCK_LABEL
    );
}

// --- The focus shortcut's session ---

const OTHER_DOCK: &str = "dock-DISPLAY3";

fn app(id: u64) -> Option<ReturnTo> {
    Some(ReturnTo::App(WindowId(id)))
}

fn session(dock: &str, window: Option<u64>) -> Option<FocusSession> {
    Some(FocusSession {
        dock: dock.to_owned(),
        window: window.map(|id| ReturnTo::App(WindowId(id))),
    })
}

#[test]
fn a_release_takes_the_session_of_its_own_dock_and_says_what_to_bring_back() {
    let focus = DockFocus::default();
    assert_eq!(
        focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
        Release::NoSession
    );

    focus.begin(DOCK_LABEL, app(7));
    assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));
    assert_eq!(
        focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
        Release::Window(ReturnTo::App(WindowId(7)))
    );
    // Taken: a second Escape has nothing to give back.
    assert_eq!(focus.session(), None);
    assert_eq!(
        focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
        Release::NoSession
    );

    // Nothing was in front at the press (the desktop, Start).
    focus.begin(DOCK_LABEL, None);
    assert_eq!(
        focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
        Release::Nothing
    );
    assert_eq!(focus.session(), None);

    // The settings window was in front at the press; its own dock's
    // release takes it whether or not the dock is still in front.
    focus.begin(DOCK_LABEL, Some(ReturnTo::Own(SETTINGS_LABEL.to_owned())));
    assert_eq!(
        focus.release(DOCK_LABEL, None),
        Release::Window(ReturnTo::Own(SETTINGS_LABEL.to_owned()))
    );
    assert_eq!(focus.session(), None);
}

#[test]
fn a_release_from_another_dock_drops_a_stale_session_and_brings_nothing_back() {
    let focus = DockFocus::default();
    // The session's dock is not in front any more (its blur has not
    // arrived): stale, whichever dock is in front.
    for front in [None, Some(DOCK_LABEL)] {
        focus.begin(OTHER_DOCK, app(7));
        assert_eq!(
            focus.release(DOCK_LABEL, front),
            Release::Stale(OTHER_DOCK.to_owned())
        );
        // Dropped, so the other dock's own Escape later goes nowhere
        // either.
        assert_eq!(focus.session(), None);
        assert_eq!(
            focus.release(OTHER_DOCK, Some(OTHER_DOCK)),
            Release::NoSession
        );
    }
}

#[test]
fn a_release_from_another_dock_leaves_the_session_of_the_dock_in_front_alone() {
    // Two monitors with `monitors = all`: the second press reached a
    // dock that does not hold the session (or any stray release came
    // from one) while the holder is still in front. The holder's own
    // release, in either order, still brings the window back.
    let focus = DockFocus::default();
    focus.begin(OTHER_DOCK, app(7));
    assert_eq!(
        focus.release(DOCK_LABEL, Some(OTHER_DOCK)),
        Release::Held(OTHER_DOCK.to_owned())
    );
    assert_eq!(focus.session(), session(OTHER_DOCK, Some(7)));
    assert_eq!(focus.holder(Some(OTHER_DOCK)), Some(OTHER_DOCK.to_owned()));
    assert_eq!(
        focus.release(OTHER_DOCK, Some(OTHER_DOCK)),
        Release::Window(ReturnTo::App(WindowId(7)))
    );
    // The late one finds nothing.
    assert_eq!(
        focus.release(DOCK_LABEL, Some(OTHER_DOCK)),
        Release::NoSession
    );
}

#[test]
fn a_later_press_replaces_the_session_whichever_dock_it_picks() {
    let focus = DockFocus::default();
    focus.begin(DOCK_LABEL, app(7));
    focus.begin(OTHER_DOCK, app(8));
    assert_eq!(focus.session(), session(OTHER_DOCK, Some(8)));
    assert_eq!(
        focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
        Release::Stale(OTHER_DOCK.to_owned())
    );
}

#[test]
fn a_press_goes_back_to_the_settings_window_when_it_is_in_front_else_to_the_engines_window() {
    // The engine's active window is the app behind the settings window:
    // it never takes Stem's own windows for the foreground.
    assert_eq!(
        return_target(Some(SETTINGS_LABEL), Some(WindowId(7))),
        Some(ReturnTo::Own(SETTINGS_LABEL.to_owned()))
    );
    assert_eq!(
        return_target(Some(SETTINGS_LABEL), None),
        Some(ReturnTo::Own(SETTINGS_LABEL.to_owned()))
    );
    // An app in front (none of our windows is).
    assert_eq!(return_target(None, Some(WindowId(7))), app(7));
    assert_eq!(return_target(None, None), None);
    // A dock in front without the session: the engine's window, as
    // before.
    assert_eq!(return_target(Some(DOCK_LABEL), Some(WindowId(7))), app(7));
    assert_eq!(return_target(Some(OTHER_DOCK), None), None);
}

#[test]
fn an_own_windows_monitor_is_the_one_with_its_bounds() {
    let left = monitor(r"\\.\DISPLAY1", false, -2560, 1.5);
    let primary = monitor(r"\\.\DISPLAY2", true, 0, 1.0);
    let monitors = vec![left.clone(), primary.clone()];
    assert_eq!(
        monitor_with_bounds(&monitors, Rect::new(-2560, 0, 2560, 1440)),
        Some(left.id)
    );
    assert_eq!(
        monitor_with_bounds(&monitors, Rect::new(0, 0, 2560, 1440)),
        Some(primary.id)
    );
    // Its work area, or a monitor the list does not know yet: none.
    assert_eq!(
        monitor_with_bounds(&monitors, Rect::new(0, 0, 2560, 1392)),
        None
    );
    assert_eq!(monitor_with_bounds(&[], Rect::new(0, 0, 2560, 1440)), None);
}

#[test]
fn a_click_on_any_dock_forgets_the_remembered_window_but_keeps_the_session() {
    let focus = DockFocus::default();
    assert_eq!(focus.supersede(), None);
    focus.begin(OTHER_DOCK, app(7));
    assert_eq!(focus.supersede(), Some(OTHER_DOCK.to_owned()));
    assert_eq!(focus.session(), session(OTHER_DOCK, None));
    // Nothing more to forget.
    assert_eq!(focus.supersede(), None);
    // The dock still has the keyboard: a second press lets go, and the
    // release brings nothing back.
    assert_eq!(focus.holder(Some(OTHER_DOCK)), Some(OTHER_DOCK.to_owned()));
    assert_eq!(
        focus.release(OTHER_DOCK, Some(OTHER_DOCK)),
        Release::Nothing
    );
    assert_eq!(focus.holder(Some(OTHER_DOCK)), None);
}

#[test]
fn a_press_lets_go_only_of_the_dock_in_front_that_holds_the_session() {
    let focus = DockFocus::default();
    // No session: every press gives a dock the focus, also with a dock
    // in front (after an Escape that had nothing to go back to).
    assert_eq!(focus.holder(None), None);
    assert_eq!(focus.holder(Some(DOCK_LABEL)), None);

    focus.begin(DOCK_LABEL, app(7));
    assert_eq!(focus.holder(Some(DOCK_LABEL)), Some(DOCK_LABEL.to_owned()));
    // Not in front (a blur that has not arrived yet), or another dock
    // in front: the press gives the focus again.
    assert_eq!(focus.holder(None), None);
    assert_eq!(focus.holder(Some(OTHER_DOCK)), None);
    // Asking changes nothing.
    assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));

    // Nothing was in front at the press: the second press still lets
    // go; after the release the third one enters again.
    focus.begin(DOCK_LABEL, None);
    assert_eq!(focus.holder(Some(DOCK_LABEL)), Some(DOCK_LABEL.to_owned()));
    assert_eq!(
        focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
        Release::Nothing
    );
    assert_eq!(focus.holder(Some(DOCK_LABEL)), None);
}

#[test]
fn only_the_docks_own_close_or_failed_activation_ends_its_session() {
    let focus = DockFocus::default();
    assert_eq!(focus.end(DOCK_LABEL), None);
    focus.begin(DOCK_LABEL, app(7));
    assert_eq!(focus.end(OTHER_DOCK), None);
    assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));
    let ended = focus.end(DOCK_LABEL);
    assert_eq!(ended, session(DOCK_LABEL, Some(7)));
    assert_eq!(focus.session(), None);
    assert_eq!(
        ended.as_ref().map(end_outcome),
        Some("the window to go back to is forgotten")
    );
    // A click had forgotten the window already: only the session ends.
    focus.begin(DOCK_LABEL, app(7));
    focus.supersede();
    assert_eq!(
        focus.end(DOCK_LABEL).as_ref().map(end_outcome),
        Some("the keyboard session ended")
    );
}

#[test]
fn a_blur_ends_the_session_only_once_its_dock_is_no_longer_in_front() {
    let focus = DockFocus::default();
    focus.begin(DOCK_LABEL, app(7));
    // Its own native menu took the focus: the dock is still in front.
    assert_eq!(focus.blurred(DOCK_LABEL, Some(DOCK_LABEL)), None);
    assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));
    // Another dock's blur leaves it alone, wherever the foreground is.
    assert_eq!(focus.blurred(OTHER_DOCK, None), None);
    assert_eq!(focus.blurred(OTHER_DOCK, Some(DOCK_LABEL)), None);
    assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));
    // Another dock took the foreground (its menu), or no dock has it (a
    // click elsewhere, Alt+Tab): the session ends, and says what it had.
    assert_eq!(
        focus.blurred(DOCK_LABEL, Some(OTHER_DOCK)),
        session(DOCK_LABEL, Some(7))
    );
    assert_eq!(focus.session(), None);
    focus.begin(DOCK_LABEL, None);
    assert_eq!(focus.blurred(DOCK_LABEL, None), session(DOCK_LABEL, None));
    assert_eq!(focus.session(), None);
    assert_eq!(focus.blurred(DOCK_LABEL, None), None, "nothing left to end");
}

#[test]
fn a_blur_after_a_click_says_that_the_session_ended_not_that_the_window_is_forgotten() {
    let focus = DockFocus::default();
    focus.begin(DOCK_LABEL, app(7));
    let ended = focus.blurred(DOCK_LABEL, None).expect("the session ends");
    assert_eq!(end_outcome(&ended), "the window to go back to is forgotten");

    // A click on an item forgot the window (its own log line); the blur
    // that follows when the clicked window comes to the front ends the
    // session and must not say that it forgot the window again.
    focus.begin(DOCK_LABEL, app(7));
    assert_eq!(focus.supersede(), Some(DOCK_LABEL.to_owned()));
    let ended = focus.blurred(DOCK_LABEL, None).expect("the session ends");
    assert_eq!(end_outcome(&ended), "the keyboard session ended");

    // Nothing was in front at the press: the same.
    focus.begin(DOCK_LABEL, None);
    let ended = focus.blurred(DOCK_LABEL, None).expect("the session ends");
    assert_eq!(end_outcome(&ended), "the keyboard session ended");
}

#[test]
fn a_failed_activation_ends_the_session_and_tells_the_page_nothing() {
    let focus = DockFocus::default();
    focus.begin(DOCK_LABEL, app(7));
    assert!(activated(&focus, DOCK_LABEL, Ok(())));
    assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));
    assert!(!activated(
        &focus,
        DOCK_LABEL,
        Err(PlatformError::Unsupported("keyboard focus"))
    ));
    assert_eq!(focus.session(), None);
    // A later press gave another dock the focus meanwhile: its session
    // stays.
    focus.begin(OTHER_DOCK, app(8));
    assert!(!activated(
        &focus,
        DOCK_LABEL,
        Err(PlatformError::Unsupported("keyboard focus"))
    ));
    assert_eq!(focus.session(), session(OTHER_DOCK, Some(8)));
}

#[test]
fn giving_the_focus_back_activates_the_remembered_window_once_and_only_for_its_dock() {
    use stem_platform::mock::{MockCall, MockPlatform};

    let platform = MockPlatform::with_demo_scenario();
    let focus = DockFocus::default();
    let activate = |id| MockCall::WindowAction(WindowId(id), WindowAction::Activate);
    let front = Some(DOCK_LABEL);
    let no_own = |own: &str| -> Result<(), String> { panic!("no own window to focus: {own}") };

    // Escape after a press from window 1.
    focus.begin(DOCK_LABEL, app(1));
    assert_eq!(
        give_focus_back(&focus, &platform, DOCK_LABEL, front, no_own),
        Ok(())
    );
    assert_eq!(platform.calls(), vec![activate(1)]);
    // A second Escape, and one with nothing in front at the press, or
    // from another dock than the session's, activate nothing.
    platform.clear_calls();
    assert_eq!(
        give_focus_back(&focus, &platform, DOCK_LABEL, front, no_own),
        Ok(())
    );
    focus.begin(DOCK_LABEL, None);
    assert_eq!(
        give_focus_back(&focus, &platform, DOCK_LABEL, front, no_own),
        Ok(())
    );
    focus.begin(OTHER_DOCK, app(1));
    assert_eq!(
        give_focus_back(&focus, &platform, DOCK_LABEL, front, no_own),
        Ok(())
    );
    assert_eq!(platform.calls(), vec![]);
    assert_eq!(focus.session(), None);

    // The remembered window closed meanwhile: the foreground stays, and
    // the command still resolves.
    focus.begin(DOCK_LABEL, app(99));
    assert_eq!(
        give_focus_back(&focus, &platform, DOCK_LABEL, front, no_own),
        Ok(())
    );
    assert_eq!(platform.calls(), vec![activate(99)]);
}

#[test]
fn a_release_from_a_dock_without_the_keyboard_never_loses_the_holders_window() {
    use stem_platform::mock::{MockCall, MockPlatform};

    // `monitors = all`: the holder is in front, and another dock's
    // page calls `release_dock_focus` first (the race of a second press
    // whose `dock://release-focus` every page took).
    let platform = MockPlatform::with_demo_scenario();
    let focus = DockFocus::default();
    let no_own = |own: &str| -> Result<(), String> { panic!("no own window to focus: {own}") };
    focus.begin(OTHER_DOCK, app(1));
    assert_eq!(
        give_focus_back(&focus, &platform, DOCK_LABEL, Some(OTHER_DOCK), no_own),
        Ok(())
    );
    assert_eq!(platform.calls(), vec![]);
    assert_eq!(
        give_focus_back(&focus, &platform, OTHER_DOCK, Some(OTHER_DOCK), no_own),
        Ok(())
    );
    assert_eq!(
        platform.calls(),
        vec![MockCall::WindowAction(WindowId(1), WindowAction::Activate)]
    );
}

#[test]
fn giving_the_focus_back_to_the_settings_window_focuses_it_and_activates_no_app() {
    use stem_platform::mock::MockPlatform;

    let platform = MockPlatform::with_demo_scenario();
    let focus = DockFocus::default();
    let focused = std::cell::RefCell::new(Vec::<String>::new());
    let focus_own = |own: &str| -> Result<(), String> {
        focused.borrow_mut().push(own.to_owned());
        Ok(())
    };
    focus.begin(DOCK_LABEL, Some(ReturnTo::Own(SETTINGS_LABEL.to_owned())));
    assert_eq!(
        give_focus_back(&focus, &platform, DOCK_LABEL, Some(DOCK_LABEL), focus_own),
        Ok(())
    );
    assert_eq!(*focused.borrow(), vec![SETTINGS_LABEL.to_owned()]);
    assert_eq!(platform.calls(), vec![]);
    assert_eq!(focus.session(), None);

    // Its failure is the command's.
    focus.begin(DOCK_LABEL, Some(ReturnTo::Own(SETTINGS_LABEL.to_owned())));
    assert_eq!(
        give_focus_back(&focus, &platform, DOCK_LABEL, Some(DOCK_LABEL), |_| Err(
            "refused".to_owned()
        )),
        Err("refused".to_owned())
    );
}
