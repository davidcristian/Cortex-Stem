//! Scrolling over an item.

use pretty_assertions::assert_eq;

use super::*;

const A_KEY: &str = "app:exe:c:/a.exe";

fn activates(id: u64) -> ScrollEffect {
    ScrollEffect::Activate(WindowId(id))
}

#[test]
fn a_scroll_cycles_the_items_windows_from_the_one_in_front_and_wraps() {
    use ScrollDirection::{Next, Previous};
    let mut engine = DockEngine::new(cycling(minimal_settings()));
    engine.apply(added(win(1, r"C:\a.exe", "A1")));
    let mut minimised = win(2, r"C:\a.exe", "A2");
    minimised.is_minimized = true;
    engine.apply(added(minimised));
    engine.apply(added(cloaked(win(9, r"C:\a.exe", "on another desktop"))));
    engine.apply(added(win(3, r"C:\a.exe", "A3")));
    engine.apply(added(win(4, r"C:\b.exe", "B")));

    // None of A's windows in front: next is the first, previous the last.
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Previous, t0()),
        activates(3)
    );
    engine.apply(foreground(Some(4)));
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Previous, t0()),
        activates(3)
    );

    // From each window in front, its neighbours in first-seen order; the
    // minimised one takes part, the cloaked one does not.
    for (front, next, previous) in [(1, 2, 3), (2, 3, 1), (3, 1, 2)] {
        engine.apply(foreground(Some(front)));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Next, t0()),
            activates(next),
            "next from {front}"
        );
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Previous, t0()),
            activates(previous),
            "previous from {front}"
        );
    }

    // A lone window in front is its own neighbour either way.
    engine.apply(foreground(Some(4)));
    for direction in [Next, Previous] {
        assert_eq!(
            engine.scroll_action("app:exe:c:/b.exe", DOCK, direction, t0()),
            activates(4)
        );
    }
}

#[test]
fn a_scroll_steps_from_the_window_the_dock_last_tried_until_the_foreground_moves() {
    use ScrollDirection::{Next, Previous};
    let mut engine = DockEngine::new(cycling(minimal_settings()));
    for id in 1..=3 {
        engine.apply(added(win(id, r"C:\a.exe", &format!("A{id}"))));
    }
    engine.apply(added(win(4, r"C:\b.exe", "B")));
    engine.apply(foreground(Some(1)));

    // Windows refuses 2 (an elevated window, say): the foreground stays
    // on 1, and the next notch goes on to 3 instead of trying 2 again;
    // back from 2 is 1.
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(2));
    engine.note_scroll_activation(DOCK, WindowId(2), t0());
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(3));
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Previous, t0()),
        activates(1)
    );
    // 3 is being activated and its report is not in yet: from 3.
    engine.note_scroll_activation(DOCK, WindowId(3), t0());
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Previous, t0()),
        activates(2)
    );
    // Each dock window has its own: another steps from the foreground.
    assert_eq!(
        engine.scroll_action(A_KEY, SECOND_DOCK, Next, t0()),
        activates(2)
    );
    // A report of the window that already was in front changes nothing.
    engine.apply(foreground(Some(1)));
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
    // 3's report: from the foreground again, which is 3.
    engine.apply(foreground(Some(3)));
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Previous, t0()),
        activates(2)
    );

    // Any change of the foreground ends it, a change and a change back
    // too (the user clicked elsewhere and returned).
    engine.note_scroll_activation(DOCK, WindowId(1), t0());
    engine.apply(foreground(Some(4)));
    engine.apply(foreground(Some(3)));
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
    // So does the foreground window's going away.
    engine.note_scroll_activation(DOCK, WindowId(1), t0());
    engine.apply(removed(3));
    assert_eq!(engine.foreground(), None);
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));

    // A tried window that is not one of the item's windows on that dock
    // (another app's, or one closed since) is passed over: from the
    // foreground, or the first when none is in front.
    engine.apply(added(win(5, r"C:\a.exe", "A5")));
    engine.apply(foreground(Some(2)));
    engine.note_scroll_activation(DOCK, WindowId(4), t0());
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(5));
    engine.note_scroll_activation(DOCK, WindowId(1), t0());
    engine.apply(removed(1));
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(5));
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Previous, t0()),
        activates(5)
    );

    // A dock window that goes takes its entry with it.
    engine.apply(panel_of(DOCK, Some(PRIMARY), None));
    engine.note_scroll_activation(DOCK, WindowId(5), t0());
    engine.note_scroll_activation(SECOND_DOCK, WindowId(5), t0());
    engine.apply(panel_of(DOCK, None, None));
    assert_eq!(
        engine.scroll_tried.keys().collect::<Vec<_>>(),
        [SECOND_DOCK]
    );
}

#[test]
fn the_window_a_scroll_tried_is_forgotten_after_the_scroll_memory() {
    use ScrollDirection::{Next, Previous};
    assert_eq!(SCROLL_MEMORY, Duration::from_secs(2));
    let mut engine = DockEngine::new(cycling(minimal_settings()));
    for id in 1..=3 {
        engine.apply(added(win(id, r"C:\a.exe", &format!("A{id}"))));
    }
    engine.apply(foreground(Some(1)));
    let at = |ms: u64| t0() + Duration::from_millis(ms);

    // Windows refuses 2: the foreground stays on 1. Within the memory
    // the dock's next notches step from 2, so 2 is passed over.
    engine.note_scroll_activation(DOCK, WindowId(2), at(0));
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Next, at(150)),
        activates(3)
    );
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Previous, at(1_999)),
        activates(1)
    );
    // A notch at the limit or later steps from the foreground again and
    // tries 2 once more.
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Next, at(2_000)),
        activates(2)
    );
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Previous, at(60_000)),
        activates(3)
    );
    // The memory runs from the last admitted notch: a new one starts it
    // again.
    engine.note_scroll_activation(DOCK, WindowId(2), at(2_000));
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Next, at(3_999)),
        activates(3)
    );
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Next, at(4_000)),
        activates(2)
    );
    // A time before the notch (never sent) counts as within it.
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Next, at(1_000)),
        activates(3)
    );
    // Each dock window's memory runs on its own.
    engine.note_scroll_activation(SECOND_DOCK, WindowId(3), at(3_000));
    assert_eq!(
        engine.scroll_action(A_KEY, SECOND_DOCK, Next, at(4_500)),
        activates(1)
    );
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Next, at(4_500)),
        activates(2)
    );
}

#[test]
fn a_scroll_does_nothing_unless_cycling_over_an_app_with_windows() {
    use ScrollDirection::{Next, Previous};
    let settings = Settings {
        pinned: vec![pin(r"C:\p.exe")],
        ..Settings::default()
    };
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(added(win(1, r"C:\a.exe", "A1")));
    engine.apply(added(win(2, r"C:\a.exe", "A2")));
    let state = engine.snapshot();
    assert!(keys(&state).contains(&"sep:1"), "{:?}", keys(&state));
    assert!(!settings.items.task_view);

    // `none`, the default: nothing anywhere, but an unknown key is still
    // an error.
    assert_eq!(settings.behavior.scroll_action, ScrollAction::None);
    for direction in [Next, Previous] {
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, direction, t0()),
            ScrollEffect::Nothing
        );
        assert_eq!(
            engine.scroll_action("nonsense", DOCK, direction, t0()),
            ScrollEffect::UnknownItem
        );
    }

    engine.apply(EngineInput::SettingsChanged(cycling(settings.clone())));
    assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
    for direction in [Next, Previous] {
        // Fixed items and separators have no windows to cycle.
        for key in ["fixed:launcher", "fixed:recycle-bin", "sep:1"] {
            assert_eq!(
                engine.scroll_action(key, DOCK, direction, t0()),
                ScrollEffect::Nothing,
                "{key}"
            );
        }
        // A pinned app without windows is not launched.
        assert_eq!(
            engine.scroll_action("app:exe:c:/p.exe", DOCK, direction, t0()),
            ScrollEffect::Nothing
        );
        // Keys of items the dock does not show.
        for key in ["nonsense", "fixed:task-view", "app:exe:c:/gone.exe"] {
            assert_eq!(
                engine.scroll_action(key, DOCK, direction, t0()),
                ScrollEffect::UnknownItem,
                "{key}"
            );
        }
    }

    // A hidden app has no item: its key is unknown, as for a click.
    let mut hiding = cycling(settings);
    hiding.hidden_apps = vec![hidden(r"C:\a.exe", "A")];
    engine.apply(EngineInput::SettingsChanged(hiding));
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Next, t0()),
        ScrollEffect::UnknownItem
    );
    assert_eq!(engine.primary_action(A_KEY, DOCK), ItemAction::None);

    // Its last window gone, an unpinned app's item is gone too.
    engine.apply(EngineInput::SettingsChanged(cycling(Settings::default())));
    engine.apply(removed(1));
    engine.apply(removed(2));
    assert_eq!(
        engine.scroll_action(A_KEY, DOCK, Next, t0()),
        ScrollEffect::UnknownItem
    );
}

#[test]
fn a_scroll_cycles_only_the_windows_the_calling_dock_lists() {
    use ScrollDirection::{Next, Previous};
    let mut engine = two_docks(true, true);
    let mut settings = cycling(engine.settings().clone());
    engine.apply(EngineInput::SettingsChanged(settings.clone()));
    // Notepad: A (1) on the primary, B (2) and C (3) on the second
    // monitor, D (4) on the primary.
    engine.apply(added(on_monitor(win(3, NOTEPAD, "C"), SECOND)));
    engine.apply(added(on_monitor(win(4, NOTEPAD, "D"), PRIMARY)));

    // A in front: the second dock has none of its windows in front.
    engine.apply(foreground(Some(1)));
    assert_eq!(
        engine.scroll_action(NOTEPAD_KEY, DOCK, Next, t0()),
        activates(4)
    );
    assert_eq!(
        engine.scroll_action(NOTEPAD_KEY, DOCK, Previous, t0()),
        activates(4)
    );
    assert_eq!(
        engine.scroll_action(NOTEPAD_KEY, SECOND_DOCK, Next, t0()),
        activates(2)
    );
    assert_eq!(
        engine.scroll_action(NOTEPAD_KEY, SECOND_DOCK, Previous, t0()),
        activates(3)
    );
    engine.apply(foreground(Some(3)));
    assert_eq!(
        engine.scroll_action(NOTEPAD_KEY, SECOND_DOCK, Next, t0()),
        activates(2)
    );
    assert_eq!(
        engine.scroll_action(NOTEPAD_KEY, DOCK, Next, t0()),
        activates(1)
    );

    // A dock with no window of the app on its monitor: nothing, and no
    // launch of the pinned app.
    engine.apply(updated(on_monitor(win(2, NOTEPAD, "B"), PRIMARY)));
    engine.apply(updated(on_monitor(win(3, NOTEPAD, "C"), PRIMARY)));
    assert_eq!(
        engine.scroll_action(NOTEPAD_KEY, SECOND_DOCK, Next, t0()),
        ScrollEffect::Nothing
    );
    assert_eq!(
        engine.scroll_action(NOTEPAD_KEY, DOCK, Next, t0()),
        activates(4)
    );

    // The setting off: every dock cycles every window, in first-seen
    // order.
    settings.behavior.show_only_current_monitor = false;
    engine.apply(EngineInput::SettingsChanged(settings));
    for dock in [DOCK, SECOND_DOCK] {
        assert_eq!(
            engine.scroll_action(NOTEPAD_KEY, dock, Next, t0()),
            activates(4),
            "{dock}"
        );
        assert_eq!(
            engine.scroll_action(NOTEPAD_KEY, dock, Previous, t0()),
            activates(2),
            "{dock}"
        );
    }
}
