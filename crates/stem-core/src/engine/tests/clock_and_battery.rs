//! The clock and battery items.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn clock_and_battery_are_off_by_default() {
    let mut engine = DockEngine::new(Settings::default());
    engine.apply(power(laptop(50)));
    let state = engine.snapshot();
    assert!(
        state
            .items
            .iter()
            .all(|i| !matches!(i.kind, DockItemKind::Clock | DockItemKind::Battery))
    );
    assert_eq!(
        keys(&state),
        vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
    );
}

#[test]
fn power_is_unknown_until_the_first_reading_and_then_kept_as_sent() {
    let mut engine = DockEngine::new(with_clock_and_battery(true, true));
    assert_eq!(engine.power(), PowerStatus::default());
    let state = engine.snapshot();
    assert_eq!(state.power, PowerStatus::default());
    assert!(!state.power.has_battery);
    assert_eq!(state.power.percent, None);
    assert!(!state.power.charging && !state.power.on_ac && !state.power.saver);

    let reading = PowerStatus {
        has_battery: true,
        percent: Some(73),
        charging: true,
        on_ac: true,
        saver: true,
    };
    assert!(engine.apply(power(reading)).changed);
    assert_eq!(engine.power(), reading);
    assert_eq!(engine.snapshot().power, reading);
}

#[test]
fn power_changed_bumps_the_revision_only_when_the_reading_differs() {
    let mut engine = DockEngine::new(with_clock_and_battery(true, true));
    assert!(engine.apply(power(laptop(76))).changed);
    let r = engine.snapshot().revision;

    // The same reading again (a resume, a repeated broadcast): nothing.
    assert!(!engine.apply(power(laptop(76))).changed);
    assert_eq!(engine.snapshot().revision, r);

    // Each field on its own is a change.
    let mut next = laptop(75);
    assert!(engine.apply(power(next)).changed);
    next.on_ac = true;
    assert!(engine.apply(power(next)).changed);
    next.charging = true;
    assert!(engine.apply(power(next)).changed);
    next.saver = true;
    assert!(engine.apply(power(next)).changed);
    next.percent = None;
    assert!(engine.apply(power(next)).changed);
    assert_eq!(engine.snapshot().revision, r + 5);

    // `DockState::power` is the platform's fact whatever the settings
    // say, but without the battery item no page shows it: the
    // snapshot carries each reading and the revision stays.
    let mut engine = DockEngine::new(Settings::default());
    let r = engine.snapshot().revision;
    assert!(!engine.apply(power(laptop(40))).changed);
    assert_eq!(engine.snapshot().power, laptop(40));
    assert!(!engine.apply(power(laptop(39))).changed);
    assert_eq!(engine.snapshot().power, laptop(39));
    assert_eq!(engine.snapshot().revision, r);
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
    );
}

#[test]
fn battery_and_clock_come_after_notification_center_and_before_the_bin() {
    let mut settings = with_clock_and_battery(true, true);
    settings.items.quick_settings = true;
    settings.items.notification_center = true;
    let mut engine = DockEngine::new(settings);
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(power(laptop(76)));
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
            "fixed:battery",
            "fixed:clock",
            "fixed:recycle-bin",
        ]
    );
    let battery = item(&state, "fixed:battery").unwrap();
    assert_eq!(battery.kind, DockItemKind::Battery);
    assert_eq!(battery.title, "Battery 76 %");
    assert_eq!(battery.icon_key, Some(IconKey("shell:battery".into())));
    let clock = item(&state, "fixed:clock").unwrap();
    assert_eq!(clock.kind, DockItemKind::Clock);
    assert_eq!(clock.title, "Clock");
    assert_eq!(clock.icon_key, Some(IconKey("shell:clock".into())));
    for item in [battery, clock] {
        assert!(item.icon_key.as_ref().is_some_and(IconKey::is_shell));
        assert_eq!(item.app_id, None);
        assert!(!item.pinned);
        assert!(item.windows.is_empty());
        assert!(!item.is_active && !item.needs_attention);
        assert_eq!(item.badge, None);
    }

    // Battery before clock, both before the bin, with the Quick Settings
    // and Notification Center items off.
    let mut engine = DockEngine::new(with_clock_and_battery(true, true));
    engine.apply(power(laptop(76)));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "fixed:battery",
            "fixed:clock",
            "fixed:recycle-bin",
        ]
    );
    // Each one alone keeps its place.
    let mut engine = DockEngine::new(with_clock_and_battery(false, true));
    engine.apply(power(laptop(76)));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "fixed:battery",
            "fixed:recycle-bin"
        ]
    );
    let mut settings = with_clock_and_battery(true, false);
    settings.items.notification_center = true;
    let engine = DockEngine::new(settings);
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "fixed:notification-center",
            "fixed:clock",
            "fixed:recycle-bin"
        ]
    );
}

#[test]
fn the_battery_item_exists_only_while_the_system_has_a_battery() {
    let mut engine = DockEngine::new(with_clock_and_battery(true, true));
    // No reading yet: no battery is known, so no battery item.
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "fixed:clock",
            "fixed:recycle-bin"
        ]
    );

    // A desktop PC: on AC, no battery. Still no battery item.
    let desktop = PowerStatus {
        has_battery: false,
        percent: None,
        charging: false,
        on_ac: true,
        saver: false,
    };
    // No item shows the reading, so it bumps nothing.
    let r = engine.snapshot().revision;
    assert!(!engine.apply(power(desktop)).changed);
    assert!(item(&engine.snapshot(), "fixed:battery").is_none());
    assert_eq!(engine.snapshot().revision, r);
    assert_eq!(engine.snapshot().power, desktop);
    // Battery saver can be on without a battery; the item stays away.
    assert!(
        !engine
            .apply(power(PowerStatus {
                saver: true,
                ..desktop
            }))
            .changed
    );
    assert!(item(&engine.snapshot(), "fixed:battery").is_none());
    assert_eq!(engine.snapshot().revision, r);

    // The system sees a battery: the item appears in its place.
    assert!(engine.apply(power(laptop(30))).changed);
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "fixed:battery",
            "fixed:clock",
            "fixed:recycle-bin"
        ]
    );
    // And goes again when it no longer does.
    assert!(engine.apply(power(desktop)).changed);
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "fixed:clock",
            "fixed:recycle-bin"
        ]
    );

    // A battery with the setting off shows nothing.
    let mut engine = DockEngine::new(Settings::default());
    engine.apply(power(laptop(30)));
    assert!(item(&engine.snapshot(), "fixed:battery").is_none());

    // The battery item alone on a desktop leaves an empty trailing group:
    // no separator for it.
    let mut alone = minimal_settings();
    alone.items.battery = true;
    let mut engine = DockEngine::new(alone);
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(power(desktop));
    assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/a.exe"]);
    engine.apply(power(laptop(30)));
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["app:exe:c:/a.exe", "sep:1", "fixed:battery"]
    );
}

#[test]
fn the_battery_title_says_the_percent_the_power_source_and_battery_saver() {
    let cases = [
        ((Some(73), false, false, false), "Battery 73 %"),
        ((Some(73), true, true, false), "Battery 73 %, charging"),
        ((Some(100), false, true, false), "Battery 100 %, plugged in"),
        ((Some(0), false, false, false), "Battery 0 %"),
        (
            (Some(18), false, false, true),
            "Battery 18 %, battery saver on",
        ),
        (
            (Some(18), true, true, true),
            "Battery 18 %, charging, battery saver on",
        ),
        (
            (Some(95), false, true, true),
            "Battery 95 %, plugged in, battery saver on",
        ),
        // Charging without AC (a system that reports it so) still says
        // charging.
        ((Some(50), true, false, false), "Battery 50 %, charging"),
        // Without a known percent the title is `Battery` alone, as the
        // contract says, whatever the rest of the reading is.
        ((None, false, false, false), "Battery"),
        ((None, true, true, false), "Battery"),
        ((None, false, true, false), "Battery"),
        ((None, false, false, true), "Battery"),
        ((None, true, true, true), "Battery"),
    ];
    for ((percent, charging, on_ac, saver), expected) in cases {
        let status = PowerStatus {
            has_battery: true,
            percent,
            charging,
            on_ac,
            saver,
        };
        assert_eq!(battery_title(&status), expected, "{status:?}");

        // The snapshot's item carries the same title.
        let mut engine = DockEngine::new(with_clock_and_battery(false, true));
        engine.apply(power(status));
        let state = engine.snapshot();
        assert_eq!(
            item(&state, "fixed:battery").map(|i| i.title.as_str()),
            Some(expected)
        );
    }

    // A discharge moves the title with the percent.
    let mut engine = DockEngine::new(with_clock_and_battery(false, true));
    engine.apply(power(laptop(76)));
    engine.apply(power(laptop(75)));
    assert_eq!(
        item(&engine.snapshot(), "fixed:battery").unwrap().title,
        "Battery 75 %"
    );
}

#[test]
fn the_clock_and_battery_follow_the_trailing_groups_separator_rules() {
    // Without the bin they close the dock, after one separator.
    let mut settings = with_clock_and_battery(true, true);
    settings.items.recycle_bin = false;
    let mut engine = DockEngine::new(settings);
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(power(laptop(60)));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "sep:1",
            "app:exe:c:/a.exe",
            "sep:2",
            "fixed:battery",
            "fixed:clock",
        ]
    );

    // Alone in the dock: no separator at all.
    let mut alone = minimal_settings();
    alone.items.clock = true;
    alone.items.battery = true;
    let mut engine = DockEngine::new(alone.clone());
    engine.apply(power(laptop(60)));
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["fixed:battery", "fixed:clock"]
    );

    // Separators off: none anywhere.
    let mut full = with_clock_and_battery(true, true);
    full.items.separators = false;
    let mut engine = DockEngine::new(full);
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(power(laptop(60)));
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "fixed:launcher",
            "app:exe:c:/a.exe",
            "fixed:battery",
            "fixed:clock",
            "fixed:recycle-bin",
        ]
    );
}

#[test]
fn switching_the_clock_or_the_battery_bumps_the_revision_only_when_it_shows() {
    let mut settings = Settings::default();
    let mut engine = DockEngine::new(settings.clone());
    let r = engine.snapshot().revision;

    // The battery item on a machine with no battery (no reading yet)
    // changes no item: no bump.
    settings.items.battery = true;
    assert!(
        !engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    assert_eq!(engine.snapshot().revision, r);

    // The reading shows it.
    assert!(engine.apply(power(laptop(80))).changed);
    assert!(item(&engine.snapshot(), "fixed:battery").is_some());

    settings.items.clock = true;
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    settings.items.battery = false;
    settings.items.clock = false;
    assert!(engine.apply(EngineInput::SettingsChanged(settings)).changed);
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
    );
    // The reading is kept across setting changes.
    assert_eq!(engine.power(), laptop(80));
}

#[test]
fn a_click_on_the_clock_or_the_battery_opens_its_windows_panel() {
    let mut engine = DockEngine::new(with_clock_and_battery(true, true));
    engine.apply(power(laptop(50)));
    assert_eq!(
        engine.primary_action("fixed:clock", DOCK),
        ItemAction::Shell(ShellAction::OpenNotificationCenter)
    );
    assert_eq!(
        engine.primary_action("fixed:battery", DOCK),
        ItemAction::Shell(ShellAction::OpenQuickSettings)
    );
    // Any dock window, as for the other fixed items.
    assert_eq!(
        engine.primary_action("fixed:clock", "dock-DISPLAY2"),
        ItemAction::Shell(ShellAction::OpenNotificationCenter)
    );
    assert_eq!(
        engine.primary_action("fixed:battery", "dock-DISPLAY2"),
        ItemAction::Shell(ShellAction::OpenQuickSettings)
    );
    // The icon keys are not item keys.
    assert_eq!(engine.primary_action("shell:clock", DOCK), ItemAction::None);
    assert_eq!(
        engine.primary_action("shell:battery", DOCK),
        ItemAction::None
    );

    // The menus are built from the items as a dock lists them.
    let clock = engine.dock_item("fixed:clock", DOCK).unwrap();
    assert_eq!(clock.kind, DockItemKind::Clock);
    let battery = engine.dock_item("fixed:battery", DOCK).unwrap();
    assert_eq!(battery.kind, DockItemKind::Battery);
    assert_eq!(battery.title, "Battery 50 %");

    // Without a battery there is no battery item to build a menu from.
    engine.apply(power(PowerStatus::default()));
    assert!(engine.dock_item("fixed:battery", DOCK).is_none());
}
