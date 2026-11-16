//! The Quick Settings and Notification Center items.

use pretty_assertions::assert_eq;

use super::*;

fn with_trailing(quick_settings: bool, notification_center: bool) -> Settings {
    let mut s = Settings::default();
    s.items.quick_settings = quick_settings;
    s.items.notification_center = notification_center;
    s
}

#[test]
fn quick_settings_and_notification_center_are_off_by_default() {
    let engine = DockEngine::new(Settings::default());
    let state = engine.snapshot();
    assert!(state.items.iter().all(|i| !matches!(
        i.kind,
        DockItemKind::QuickSettings | DockItemKind::NotificationCenter
    )));
}

#[test]
fn quick_settings_and_notification_center_come_before_the_recycle_bin_in_that_order() {
    let mut engine = DockEngine::new(with_trailing(true, true));
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    let state = engine.snapshot();
    assert_eq!(
        keys(&state),
        vec![
            "fixed:launcher",
            "sep:1",
            "app:exe:c:/a.exe",
            "sep:2",
            "fixed:quick-settings",
            "fixed:notification-center",
            "fixed:recycle-bin",
        ]
    );
    let qs = &state.items[4];
    assert_eq!(qs.kind, DockItemKind::QuickSettings);
    assert_eq!(qs.title, "Quick Settings");
    assert_eq!(qs.icon_key, Some(IconKey("shell:quick-settings".into())));
    let nc = &state.items[5];
    assert_eq!(nc.kind, DockItemKind::NotificationCenter);
    assert_eq!(nc.title, "Notification Center");
    assert_eq!(
        nc.icon_key,
        Some(IconKey("shell:notification-center".into()))
    );
    for item in [qs, nc] {
        assert!(item.icon_key.as_ref().is_some_and(IconKey::is_shell));
        assert_eq!(item.app_id, None);
        assert!(!item.pinned);
        assert!(item.windows.is_empty());
        assert!(!item.is_active && !item.needs_attention);
    }

    // Each one alone keeps its place before the bin.
    let engine = DockEngine::new(with_trailing(true, false));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "fixed:quick-settings",
            "fixed:recycle-bin"
        ]
    );
    let engine = DockEngine::new(with_trailing(false, true));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "fixed:notification-center",
            "fixed:recycle-bin"
        ]
    );
}

#[test]
fn the_trailing_group_has_one_separator_before_it_and_none_inside() {
    // Without the bin the two still close the dock, after one separator.
    let mut settings = with_trailing(true, true);
    settings.items.recycle_bin = false;
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "app:exe:c:/a.exe",
            "sep:2",
            "fixed:quick-settings",
            "fixed:notification-center",
        ]
    );

    // Alone in the dock: no separator at all.
    let mut alone = minimal_settings();
    alone.items.quick_settings = true;
    alone.items.notification_center = true;
    let engine = DockEngine::new(alone);
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["fixed:quick-settings", "fixed:notification-center"]
    );

    // With pinned and running apps: the same separators as before, the
    // last one before the whole trailing group.
    let mut full = with_trailing(true, true);
    full.pinned = vec![pin(r"C:\p.exe")];
    let mut engine = DockEngine::new(full.clone());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "app:exe:c:/p.exe",
            "sep:2",
            "app:exe:c:/a.exe",
            "sep:3",
            "fixed:quick-settings",
            "fixed:notification-center",
            "fixed:recycle-bin",
        ]
    );

    // Separators off: none anywhere.
    full.items.separators = false;
    engine.apply(EngineInput::SettingsChanged(full));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "app:exe:c:/p.exe",
            "app:exe:c:/a.exe",
            "fixed:quick-settings",
            "fixed:notification-center",
            "fixed:recycle-bin",
        ]
    );
}

#[test]
fn switching_quick_settings_or_notification_center_bumps_the_revision() {
    let mut settings = Settings::default();
    let mut engine = DockEngine::new(settings.clone());
    let r = engine.snapshot().revision;

    settings.items.quick_settings = true;
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    assert!(engine.snapshot().revision > r);
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "fixed:quick-settings",
            "fixed:recycle-bin"
        ]
    );

    settings.items.notification_center = true;
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    settings.items.quick_settings = false;
    settings.items.notification_center = false;
    assert!(engine.apply(EngineInput::SettingsChanged(settings)).changed);
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
    );
}

#[test]
fn a_click_on_quick_settings_or_notification_center_runs_its_shell_action() {
    let engine = DockEngine::new(with_trailing(true, true));
    assert_eq!(
        engine.primary_action("fixed:quick-settings", DOCK),
        ItemAction::Shell(ShellAction::OpenQuickSettings)
    );
    assert_eq!(
        engine.primary_action("fixed:notification-center", DOCK),
        ItemAction::Shell(ShellAction::OpenNotificationCenter)
    );
    // Any dock window, as for the other fixed items.
    assert_eq!(
        engine.primary_action("fixed:quick-settings", "dock-DISPLAY3"),
        ItemAction::Shell(ShellAction::OpenQuickSettings)
    );
    // The bin keeps its own action; the icon keys are not item keys.
    assert_eq!(
        engine.primary_action("fixed:recycle-bin", DOCK),
        ItemAction::Shell(ShellAction::OpenRecycleBin)
    );
    assert_eq!(
        engine.primary_action("shell:quick-settings", DOCK),
        ItemAction::None
    );

    // The menu is built from the item as a dock lists it: the snapshot's.
    let item = engine.dock_item("fixed:notification-center", DOCK).unwrap();
    assert_eq!(item.kind, DockItemKind::NotificationCenter);
    assert!(item.windows.is_empty());
}
