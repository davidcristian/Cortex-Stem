//! The snapshot's groups and order, the active and attention marks, clicks,
//! identity requests and the revision.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn empty_engine_shows_only_fixed_items() {
    let engine = DockEngine::new(Settings::default());
    let state = engine.snapshot();
    assert_eq!(
        keys(&state),
        vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
    );
}

#[test]
fn windows_are_grouped_by_app_and_ordered_by_first_seen() {
    let mut engine = DockEngine::new(minimal_settings());
    let out = engine.apply(added(win(1, r"C:\b.exe", "B1")));
    assert!(out.changed);
    assert_eq!(out.unresolved_apps, vec![exe_id(r"C:\b.exe")]);
    engine.apply(added(win(2, r"C:\a.exe", "A1")));
    engine.apply(added(win(3, r"C:\b.exe", "B2")));

    let state = engine.snapshot();
    assert_eq!(keys(&state), vec!["app:exe:c:/b.exe", "app:exe:c:/a.exe"]);
    assert_eq!(state.items[0].windows.len(), 2);
    assert_eq!(state.items[0].title, "B1");
}

#[test]
fn pinned_apps_come_first_and_absorb_their_windows() {
    let mut settings = minimal_settings();
    settings.pinned = vec![pin(r"C:\a.exe"), pin(r"C:\z.exe")];
    let mut engine = DockEngine::new(settings);
    engine.apply(added(win(1, r"C:\b.exe", "B")));
    engine.apply(added(win(2, r"C:\a.exe", "A")));

    let state = engine.snapshot();
    assert_eq!(
        keys(&state),
        vec![
            "app:exe:c:/a.exe",
            "app:exe:c:/z.exe",
            "sep:1",
            "app:exe:c:/b.exe"
        ]
    );
    assert!(state.items[0].pinned);
    assert_eq!(state.items[0].windows.len(), 1);
    assert!(state.items[1].windows.is_empty());
    assert!(!state.items[3].pinned);
}

#[test]
fn foreground_marks_active_and_clears_attention() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(attention(1, true));
    assert!(engine.snapshot().items[0].needs_attention);
    let out = engine.apply(foreground(Some(1)));
    assert!(out.changed);
    let item = &engine.snapshot().items[0];
    assert!(item.is_active);
    assert!(!item.needs_attention);
}

#[test]
fn a_flash_of_the_foreground_window_does_not_ask_for_attention() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(added(win(2, r"C:\b.exe", "B")));
    engine.apply(foreground(Some(1)));
    let before = engine.snapshot();

    // The window in front flashes: nothing to show, nothing to clear later.
    assert!(!engine.apply(attention(1, true)).changed);
    assert_eq!(engine.snapshot(), before);

    // A window behind it still asks for attention ...
    assert!(engine.apply(attention(2, true)).changed);
    assert!(engine.snapshot().items[1].needs_attention);

    // ... and once the first window is no longer in front, so does it.
    engine.apply(foreground(Some(2)));
    assert!(engine.apply(attention(1, true)).changed);
    let state = engine.snapshot();
    assert!(state.items[0].needs_attention);
    assert!(!state.items[1].needs_attention);
}

#[test]
fn cloaked_windows_are_hidden_by_default() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(cloaked(win(1, r"C:\a.exe", "A"))));
    assert!(engine.snapshot().items.is_empty());
}

#[test]
fn snapshot_resync_preserves_order_and_drops_vanished() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(added(win(2, r"C:\b.exe", "B")));
    engine.apply(
        PlatformEvent::WindowsSnapshot(vec![win(2, r"C:\b.exe", "B"), win(3, r"C:\c.exe", "C")])
            .into(),
    );
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["app:exe:c:/b.exe", "app:exe:c:/c.exe"]
    );
}

#[test]
fn primary_action_semantics() {
    let mut settings = minimal_settings();
    settings.pinned = vec![pin(r"C:\p.exe")];
    let mut engine = DockEngine::new(settings);
    let p = exe_id(r"C:\p.exe");

    // Pinned, not running -> launch.
    assert_eq!(
        engine.primary_action("app:exe:c:/p.exe", DOCK),
        ItemAction::Launch(p)
    );

    // One window, not active -> activate.
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::ActivateWindow(WindowId(1))
    );

    // One window, active -> minimise.
    engine.apply(foreground(Some(1)));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::MinimizeWindow(WindowId(1))
    );

    // Two windows, first active -> cycle to the second.
    engine.apply(added(win(2, r"C:\a.exe", "A2")));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::ActivateWindow(WindowId(2))
    );

    // Unknown running app with no launch info -> nothing after it closes.
    engine.apply(removed(1));
    engine.apply(removed(2));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::None
    );

    // Fixed items.
    assert_eq!(
        engine.primary_action("fixed:launcher", DOCK),
        ItemAction::Shell(ShellAction::OpenStartMenu)
    );
    assert_eq!(engine.primary_action("nonsense", DOCK), ItemAction::None);
}

#[test]
fn primary_action_keeps_the_active_window_when_click_active_minimizes_is_off() {
    let mut settings = minimal_settings();
    settings.behavior.click_active_minimizes = false;
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(foreground(Some(1)));

    // One window, active -> activated again, not minimised.
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::ActivateWindow(WindowId(1))
    );

    // Several windows still cycle, as with the setting on.
    engine.apply(added(win(2, r"C:\a.exe", "A2")));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::ActivateWindow(WindowId(2))
    );

    // The setting is read at click time: turning it on minimises again.
    engine.apply(removed(2));
    settings.behavior.click_active_minimizes = true;
    engine.apply(EngineInput::SettingsChanged(settings));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::MinimizeWindow(WindowId(1))
    );
}

#[test]
fn identity_is_requested_once_until_resolved() {
    let mut engine = DockEngine::new(minimal_settings());
    let a = exe_id(r"C:\a.exe");
    let out = engine.apply(added(win(1, r"C:\a.exe", "A")));
    assert_eq!(out.unresolved_apps, vec![a.clone()]);
    let out = engine.apply(added(win(2, r"C:\a.exe", "A2")));
    assert!(out.unresolved_apps.is_empty());
    assert_eq!(engine.unresolved_apps(), vec![a.clone()]);

    let out = engine.apply(EngineInput::IdentityResolved {
        app_id: a.clone(),
        identity: identity(&a, "App A", "k"),
    });
    assert!(out.changed);
    assert!(engine.unresolved_apps().is_empty());
    let item = &engine.snapshot().items[0];
    assert_eq!(item.title, "App A");
    assert_eq!(item.icon_key, Some(IconKey("k".into())));
}

#[test]
fn revision_only_advances_on_change() {
    let mut engine = DockEngine::new(minimal_settings());
    let r0 = engine.snapshot().revision;
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    let r1 = engine.snapshot().revision;
    assert!(r1 > r0);
    let out = engine.apply(updated(win(1, r"C:\a.exe", "A")));
    assert!(!out.changed);
    assert_eq!(engine.snapshot().revision, r1);
}
