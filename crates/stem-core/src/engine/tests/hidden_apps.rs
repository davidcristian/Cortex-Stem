//! Hidden apps.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn a_hidden_app_has_no_item_and_comes_back_when_shown() {
    let mut settings = minimal_settings();
    settings.pinned = vec![pin(r"C:\p.exe")];
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(added(win(2, r"C:\b.exe", "B")));
    engine.apply(added(win(3, r"C:\a.exe", "A2")));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "app:exe:c:/p.exe",
            "sep:1",
            "app:exe:c:/a.exe",
            "app:exe:c:/b.exe"
        ]
    );

    // Hide A: its item goes (one bump), B keeps its place.
    settings.hidden_apps = vec![hidden(r"C:\a.exe", "A")];
    let r = engine.snapshot().revision;
    let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
    assert!(out.changed);
    assert_eq!(engine.snapshot().revision, r + 1);
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["app:exe:c:/p.exe", "sep:1", "app:exe:c:/b.exe"]
    );

    // A second window of A brings no item either, and changes nothing
    // the snapshot shows.
    let out = engine.apply(added(win(4, r"C:\a.exe", "A3")));
    assert!(!out.changed);
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["app:exe:c:/p.exe", "sep:1", "app:exe:c:/b.exe"]
    );

    // With B hidden too, no running group is left and so no separator.
    settings.hidden_apps.push(hidden(r"C:\b.exe", "B"));
    engine.apply(EngineInput::SettingsChanged(settings.clone()));
    assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/p.exe"]);

    // Shown again: A is back where first-seen order puts it, with all
    // its windows, the one added while hidden included.
    settings.hidden_apps = vec![hidden(r"C:\b.exe", "B")];
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    let state = engine.snapshot();
    assert_eq!(
        keys(&state),
        vec!["app:exe:c:/p.exe", "sep:1", "app:exe:c:/a.exe"]
    );
    assert_eq!(ids(&state.items[2].windows), vec![1, 3, 4]);
    settings.hidden_apps.clear();
    engine.apply(EngineInput::SettingsChanged(settings));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "app:exe:c:/p.exe",
            "sep:1",
            "app:exe:c:/a.exe",
            "app:exe:c:/b.exe"
        ]
    );
}

#[test]
fn a_hidden_apps_windows_stay_tracked() {
    let mut settings = minimal_settings();
    settings.hidden_apps = vec![hidden(r"C:\a.exe", "A")];
    let mut engine = DockEngine::new(settings);
    engine.apply(monitors(vec![monitor(PRIMARY, true, 0)]));
    engine.apply(panel(Some(PANEL)));
    engine.apply(added(framed(1, r"C:\a.exe", OVER)));
    engine.apply(added(win(2, r"C:\b.exe", "B")));
    let a = exe_id(r"C:\a.exe");

    // No item, but the window is known: its frame over the panel
    // overlaps the dock, and it can be the foreground window, with no
    // item active for it.
    assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/b.exe"]);
    assert!(engine.window(WindowId(1)).is_some());
    assert_eq!(engine.windows_of(&a), vec![WindowId(1)]);
    assert!(is_overlapped(&engine, PRIMARY));
    assert!(engine.apply(foreground(Some(1))).changed);
    let state = engine.snapshot();
    assert_eq!(state.foreground, Some(WindowId(1)));
    assert!(state.items.iter().all(|i| !i.is_active));

    // Fullscreen is the platform's verdict, whoever's window it is.
    engine.apply(fullscreen_on(Some(PRIMARY)));
    assert!(is_fullscreen(&engine, PRIMARY));

    // Its identity is still asked for, and a click on its key (from a
    // page a state behind) does nothing.
    assert!(engine.unresolved_apps().contains(&a));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::None
    );
    assert_eq!(engine.dock_item("app:exe:c:/a.exe", DOCK), None);

    // Its window closing changes the overlap like any other.
    engine.apply(removed(1));
    assert!(!is_overlapped(&engine, PRIMARY));
    assert_eq!(engine.snapshot().foreground, None);
}

#[test]
fn a_pinned_app_is_never_hidden_even_by_an_unvalidated_list() {
    let mut settings = minimal_settings();
    settings.pinned = vec![pin(r"C:\p.exe")];
    // Not validated: the engine itself keeps the pin.
    settings.hidden_apps = vec![hidden(r"C:\p.exe", "P")];
    let mut engine = DockEngine::new(settings);
    engine.apply(added(win(1, r"C:\p.exe", "P")));
    let state = engine.snapshot();
    assert_eq!(keys(&state), vec!["app:exe:c:/p.exe"]);
    assert!(state.items[0].pinned);
    assert_eq!(ids(&state.items[0].windows), vec![1]);
    assert_eq!(
        engine.primary_action("app:exe:c:/p.exe", DOCK),
        ItemAction::ActivateWindow(WindowId(1))
    );
}

#[test]
fn a_hidden_app_has_no_item_on_any_dock() {
    let mut engine = two_docks(false, true);
    let mut settings = engine.settings().clone();
    settings.hidden_apps = vec![hidden(NOTEPAD, "Notepad")];
    engine.apply(EngineInput::SettingsChanged(settings));
    for dock in [DOCK, SECOND_DOCK] {
        assert_eq!(engine.dock_item(NOTEPAD_KEY, dock), None, "{dock}");
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, dock),
            ItemAction::None,
            "{dock}"
        );
    }
    assert!(engine.snapshot().items.is_empty());
    // Still tracked on both monitors.
    assert_eq!(
        engine.windows_on_dock(&exe_id(NOTEPAD), SECOND_DOCK),
        vec![WindowId(2)]
    );
}

#[test]
fn app_name_is_the_identity_else_the_first_windows_title() {
    let mut engine = DockEngine::new(minimal_settings());
    let a = exe_id(r"C:\a.exe");
    assert_eq!(engine.app_name(&a), None, "unknown app");
    engine.apply(added(cloaked(win(4, r"C:\a.exe", "first, cloaked"))));
    // No shown window, so no item: the first-seen window names it.
    assert_eq!(engine.app_name(&a).as_deref(), Some("first, cloaked"));
    engine.apply(added(win(3, r"C:\a.exe", "second")));
    engine.apply(added(win(2, r"C:\a.exe", "third")));
    // The title the item shows: its first shown window's, in first-seen
    // order (a lower window id does not matter), not the cloaked one's.
    let title = |engine: &DockEngine| {
        engine
            .snapshot()
            .items
            .into_iter()
            .find(|i| i.app_id.as_ref() == Some(&a))
            .map(|i| i.title)
    };
    assert_eq!(title(&engine).as_deref(), Some("second"));
    assert_eq!(engine.app_name(&a), title(&engine));
    engine.apply(removed(3));
    assert_eq!(engine.app_name(&a).as_deref(), Some("third"));
    assert_eq!(engine.app_name(&a), title(&engine));
    engine.apply(EngineInput::IdentityResolved {
        app_id: a.clone(),
        identity: identity(&a, "App A", "icon:a"),
    });
    assert_eq!(engine.app_name(&a).as_deref(), Some("App A"));
    assert_eq!(engine.app_name(&a), title(&engine));
    // The identity outlives the windows.
    engine.apply(removed(2));
    engine.apply(removed(4));
    assert_eq!(engine.app_name(&a).as_deref(), Some("App A"));
}

#[test]
fn app_window_is_the_first_shown_window_else_the_first_seen_one() {
    let mut engine = DockEngine::new(minimal_settings());
    let a = exe_id(r"C:\a.exe");
    assert_eq!(engine.app_window(&a), None, "no window");
    engine.apply(added(cloaked(win(4, r"C:\a.exe", "first, cloaked"))));
    engine.apply(added(cloaked(win(5, r"C:\a.exe", "second, cloaked"))));
    assert_eq!(engine.app_window(&a).map(|w| w.id), Some(WindowId(4)));
    engine.apply(added(win(3, r"C:\a.exe", "shown")));
    assert_eq!(engine.app_window(&a).map(|w| w.id), Some(WindowId(3)));
    assert_eq!(
        engine.app_window(&a).and_then(|w| w.exe_path.clone()),
        Some(std::path::PathBuf::from(r"C:\a.exe"))
    );
    // An identity does not stand in for a window.
    engine.apply(EngineInput::IdentityResolved {
        app_id: a.clone(),
        identity: identity(&a, "App A", "icon:a"),
    });
    for id in [3, 4, 5] {
        engine.apply(removed(id));
    }
    assert_eq!(engine.app_window(&a), None);
    assert_eq!(engine.app_name(&a).as_deref(), Some("App A"));
}
