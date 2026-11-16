//! Clicks and menus act on the windows the dock shows.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn a_click_acts_on_the_windows_on_the_docks_own_monitor() {
    let mut engine = two_docks(false, true);
    let notepad = exe_id(NOTEPAD);
    // A cloaked window on the second monitor is hidden there as anywhere.
    engine.apply(added(on_monitor(cloaked(win(9, NOTEPAD, "away")), SECOND)));
    assert_eq!(
        engine.windows_on_dock(&notepad, SECOND_DOCK),
        vec![WindowId(2)]
    );
    assert_eq!(engine.windows_on_dock(&notepad, DOCK), vec![WindowId(1)]);

    // Nothing active: each dock activates its own window, not the
    // app's first-seen one.
    assert_eq!(
        engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
        ItemAction::ActivateWindow(WindowId(2))
    );
    assert_eq!(
        engine.primary_action(NOTEPAD_KEY, DOCK),
        ItemAction::ActivateWindow(WindowId(1))
    );

    // B active: its dock minimises it instead of cycling to A; the
    // primary's dock, which shows only A, activates A.
    engine.apply(foreground(Some(2)));
    assert_eq!(
        engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
        ItemAction::MinimizeWindow(WindowId(2))
    );
    assert_eq!(
        engine.primary_action(NOTEPAD_KEY, DOCK),
        ItemAction::ActivateWindow(WindowId(1))
    );

    // Without clickActiveMinimizes the active window is activated again.
    let mut settings = engine.settings().clone();
    settings.behavior.click_active_minimizes = false;
    engine.apply(EngineInput::SettingsChanged(settings.clone()));
    assert_eq!(
        engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
        ItemAction::ActivateWindow(WindowId(2))
    );
    settings.behavior.click_active_minimizes = true;
    engine.apply(EngineInput::SettingsChanged(settings));

    // Two windows on one monitor cycle among themselves, skipping A.
    engine.apply(added(on_monitor(win(3, NOTEPAD, "C"), SECOND)));
    assert_eq!(
        engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
        ItemAction::ActivateWindow(WindowId(3))
    );
    engine.apply(foreground(Some(3)));
    assert_eq!(
        engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
        ItemAction::ActivateWindow(WindowId(2))
    );

    // A window moved to the other monitor follows at once.
    engine.apply(updated(on_monitor(win(1, NOTEPAD, "A"), SECOND)));
    assert_eq!(
        engine.windows_on_dock(&notepad, SECOND_DOCK),
        vec![WindowId(1), WindowId(2), WindowId(3)]
    );
    assert!(engine.windows_on_dock(&notepad, DOCK).is_empty());

    // Fixed items are the same on every dock.
    assert_eq!(
        engine.primary_action("fixed:launcher", SECOND_DOCK),
        ItemAction::Shell(ShellAction::OpenStartMenu)
    );
}

#[test]
fn a_docks_menu_item_lists_only_the_windows_on_its_monitor() {
    let mut engine = two_docks(false, true);
    engine.apply(foreground(Some(1)));
    engine.apply(attention(2, true));

    // The second dock: B only; A is active, B asks for attention.
    let item = engine.dock_item(NOTEPAD_KEY, SECOND_DOCK).unwrap();
    assert_eq!(ids(&item.windows), vec![2]);
    assert!(!item.is_active);
    assert!(item.needs_attention);
    assert_eq!(item.title, "A");
    // The primary's: A only, active, and no attention: only B flashed.
    let item = engine.dock_item(NOTEPAD_KEY, DOCK).unwrap();
    assert_eq!(ids(&item.windows), vec![1]);
    assert!(item.is_active);
    assert!(!item.needs_attention);

    // A dock that lists none of the windows keeps the item, without
    // windows, active state or attention.
    engine.apply(updated(on_monitor(win(1, NOTEPAD, "A"), SECOND)));
    let item = engine.dock_item(NOTEPAD_KEY, DOCK).unwrap();
    assert!(item.windows.is_empty());
    assert!(!item.is_active);
    assert!(!item.needs_attention);

    // Other items are the snapshot's; unknown keys have none.
    let mut settings = engine.settings().clone();
    settings.items.launcher = true;
    engine.apply(EngineInput::SettingsChanged(settings));
    let snapshot = engine.snapshot();
    assert_eq!(
        engine.dock_item("fixed:launcher", SECOND_DOCK).as_ref(),
        snapshot.items.iter().find(|i| i.key == "fixed:launcher")
    );
    assert_eq!(engine.dock_item("app:exe:c:/nope.exe", SECOND_DOCK), None);
}

#[test]
fn a_pinned_app_with_no_window_on_the_docks_monitor_is_launched_there() {
    let mut engine = two_docks(true, true);
    engine.apply(removed(1));
    // Only B is left, on the second monitor: the primary's dock shows no
    // window for Notepad and launches it; the second dock activates B.
    assert_eq!(
        engine.primary_action(NOTEPAD_KEY, DOCK),
        ItemAction::Launch(exe_id(NOTEPAD))
    );
    assert_eq!(
        engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
        ItemAction::ActivateWindow(WindowId(2))
    );
    let item = engine.dock_item(NOTEPAD_KEY, DOCK).unwrap();
    assert!(item.pinned);
    assert!(item.windows.is_empty());

    // Unpinned and without a launch target: nothing, as for a closed app.
    let mut engine = two_docks(false, true);
    engine.apply(removed(1));
    assert_eq!(engine.primary_action(NOTEPAD_KEY, DOCK), ItemAction::None);
}

#[test]
fn every_window_counts_with_one_dock_the_setting_off_or_an_unknown_monitor() {
    let notepad = exe_id(NOTEPAD);
    let every = vec![WindowId(1), WindowId(2)];
    let assert_unnarrowed = |engine: &DockEngine, dock: &str| {
        assert_eq!(engine.windows_on_dock(&notepad, dock), every, "{dock}");
        assert_eq!(
            engine.windows_on_dock(&notepad, dock),
            engine.windows_of(&notepad),
            "{dock}"
        );
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, dock),
            ItemAction::ActivateWindow(WindowId(1)),
            "{dock}"
        );
        let item = engine.dock_item(NOTEPAD_KEY, dock).unwrap();
        assert_eq!(ids(&item.windows), vec![1, 2], "{dock}");
    };

    // The setting off: every dock lists everything.
    let mut engine = two_docks(false, false);
    for dock in [DOCK, SECOND_DOCK, "dock-DISPLAY9"] {
        assert_unnarrowed(&engine, dock);
    }

    // On, but with one dock (`monitors = primary`): today's behaviour,
    // whatever the app layer last registered.
    let mut settings = engine.settings().clone();
    settings.behavior.show_only_current_monitor = true;
    settings.monitors = MonitorMode::Primary;
    engine.apply(EngineInput::SettingsChanged(settings.clone()));
    for dock in [DOCK, SECOND_DOCK] {
        assert_unnarrowed(&engine, dock);
    }

    // `monitors = all` again: narrowed, read at click time.
    settings.monitors = MonitorMode::All;
    engine.apply(EngineInput::SettingsChanged(settings));
    assert_eq!(
        engine.windows_on_dock(&notepad, SECOND_DOCK),
        vec![WindowId(2)]
    );

    // A label the app layer never registered, and a dock window it
    // could not place: every window.
    assert_unnarrowed(&engine, "dock-DISPLAY9");
    engine.apply(panel_of(
        SECOND_DOCK,
        None,
        Some(Rect::new(3000, 1000, 560, 72)),
    ));
    assert_eq!(engine.dock_monitor(SECOND_DOCK), None);
    assert_unnarrowed(&engine, SECOND_DOCK);

    // A window whose monitor the platform could not tell is on no
    // dock's monitor, as on the page.
    engine.apply(updated(win(1, NOTEPAD, "A")));
    assert!(engine.windows_on_dock(&notepad, DOCK).is_empty());
    assert_eq!(engine.windows_of(&notepad), every);
}
