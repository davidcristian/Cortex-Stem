//! The volume item.

use pretty_assertions::assert_eq;

use super::*;

fn with_volume(on: bool) -> Settings {
    let mut s = Settings::default();
    s.items.volume = on;
    s
}

fn level(percent: u8, muted: bool) -> VolumeStatus {
    VolumeStatus { percent, muted }
}

fn volume(status: Option<VolumeStatus>) -> EngineInput {
    PlatformEvent::VolumeChanged(status).into()
}

#[test]
fn the_volume_item_is_off_by_default_and_its_reading_is_kept_whatever_the_settings() {
    let mut engine = DockEngine::new(Settings::default());
    assert_eq!(engine.volume(), None);
    assert_eq!(engine.snapshot().volume, None);
    // The first reading reaches the snapshot even with the item off,
    // without a new revision, as no item shows it.
    assert!(!engine.apply(volume(Some(level(60, false)))).changed);
    assert_eq!(engine.volume(), Some(level(60, false)));
    assert_eq!(engine.snapshot().volume, Some(level(60, false)));
    assert!(
        engine
            .snapshot()
            .items
            .iter()
            .all(|i| i.kind != DockItemKind::Volume)
    );
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
    );
}

#[test]
fn volume_changed_bumps_the_revision_only_when_the_reading_differs() {
    let mut engine = DockEngine::new(with_volume(true));
    let r = engine.snapshot().revision;
    // "No device" before the first reading: the same again is nothing.
    assert!(!engine.apply(volume(None)).changed);
    assert_eq!(engine.snapshot().revision, r);

    assert!(engine.apply(volume(Some(level(60, false)))).changed);
    assert!(!engine.apply(volume(Some(level(60, false)))).changed);
    assert!(engine.apply(volume(Some(level(62, false)))).changed);
    assert!(engine.apply(volume(Some(level(62, true)))).changed);
    assert!(engine.apply(volume(None)).changed);
    assert!(!engine.apply(volume(None)).changed);
    assert_eq!(engine.snapshot().revision, r + 4);

    // The item off: still part of the snapshot, but no item shows it,
    // so no bump.
    let mut engine = DockEngine::new(Settings::default());
    let r = engine.snapshot().revision;
    assert!(!engine.apply(volume(Some(level(10, false)))).changed);
    assert!(!engine.apply(volume(Some(level(10, true)))).changed);
    assert!(!engine.apply(volume(None)).changed);
    assert_eq!(engine.snapshot().volume, None);
    assert_eq!(engine.snapshot().revision, r);
}

#[test]
fn a_reading_no_shown_item_uses_is_kept_without_a_new_revision() {
    // The battery and the volume item off, readings come in.
    let mut engine = DockEngine::new(Settings::default());
    let r = engine.snapshot().revision;
    for percent in [76, 75, 74] {
        assert!(!engine.apply(power(laptop(percent))).changed);
    }
    for percent in [60, 61, 62] {
        assert!(!engine.apply(volume(Some(level(percent, false)))).changed);
    }
    assert_eq!(engine.snapshot().revision, r);
    // The next state sent for another reason carries the latest of both.
    assert!(engine.apply(added(win(1, r"C:\a.exe", "A"))).changed);
    let state = engine.snapshot();
    assert_eq!(state.revision, r + 1);
    assert_eq!(state.power, laptop(74));
    assert_eq!(state.volume, Some(level(62, false)));

    // An item switched on shows the reading kept at once.
    let mut settings = engine.settings().clone();
    settings.items.battery = true;
    settings.items.volume = true;
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    let state = engine.snapshot();
    assert_eq!(item(&state, "fixed:battery").unwrap().title, "Battery 74 %");
    assert_eq!(item(&state, "fixed:volume").unwrap().title, "Volume 62 %");
    // While they are shown, every new reading of either is a change.
    let r = state.revision;
    assert!(engine.apply(power(laptop(73))).changed);
    assert!(engine.apply(volume(Some(level(62, true)))).changed);
    assert_eq!(engine.snapshot().revision, r + 2);

    // Each one counts on its own item only: the volume item off, a new
    // volume reading is nothing while the battery item still shows the
    // power, and the other way round.
    settings.items.volume = false;
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    assert!(!engine.apply(volume(Some(level(30, false)))).changed);
    assert!(engine.apply(power(laptop(72))).changed);
    settings.items.volume = true;
    settings.items.battery = false;
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    assert_eq!(
        item(&engine.snapshot(), "fixed:volume").unwrap().title,
        "Volume 30 %"
    );
    assert!(!engine.apply(power(laptop(71))).changed);
    assert!(engine.apply(volume(Some(level(31, false)))).changed);

    // The battery item on but no system battery: no item shows the
    // power, so battery saver toggling on a desktop is nothing either,
    // and a battery that appears is a change (its item comes).
    settings.items.battery = true;
    let desktop = PowerStatus {
        has_battery: false,
        percent: None,
        charging: false,
        on_ac: true,
        saver: false,
    };
    assert!(!engine.apply(power(desktop)).changed);
    assert!(!engine.apply(EngineInput::SettingsChanged(settings)).changed);
    assert!(item(&engine.snapshot(), "fixed:battery").is_none());
    let r = engine.snapshot().revision;
    assert!(
        !engine
            .apply(power(PowerStatus {
                saver: true,
                ..desktop
            }))
            .changed
    );
    assert_eq!(engine.snapshot().revision, r);
    assert!(engine.apply(power(laptop(70))).changed);
    assert_eq!(engine.snapshot().revision, r + 1);
    assert!(item(&engine.snapshot(), "fixed:battery").is_some());
    // And one that goes is a change too (its item goes).
    assert!(engine.apply(power(desktop)).changed);
    assert!(item(&engine.snapshot(), "fixed:battery").is_none());
}

#[test]
fn the_volume_item_comes_after_notification_center_and_before_the_battery() {
    let mut settings = with_clock_and_battery(true, true);
    settings.items.quick_settings = true;
    settings.items.notification_center = true;
    settings.items.volume = true;
    let mut engine = DockEngine::new(settings);
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(power(laptop(76)));
    engine.apply(volume(Some(level(45, false))));
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
            "fixed:volume",
            "fixed:battery",
            "fixed:clock",
            "fixed:recycle-bin",
        ]
    );
    let volume_item = item(&state, "fixed:volume").unwrap();
    assert_eq!(volume_item.kind, DockItemKind::Volume);
    assert_eq!(volume_item.title, "Volume 45 %");
    assert_eq!(volume_item.icon_key, Some(IconKey("shell:volume".into())));
    assert_eq!(volume_item.app_id, None);
    assert!(!volume_item.pinned && volume_item.windows.is_empty());
    assert!(!volume_item.is_active && !volume_item.needs_attention);

    // Shown without a device too (its face says so), alone in its
    // group after one separator, and closing the dock without the bin.
    let mut alone = with_volume(true);
    alone.items.recycle_bin = false;
    assert_eq!(
        keys(&DockEngine::new(alone).snapshot()),
        vec!["fixed:launcher", "sep:1", "fixed:volume"]
    );
    let mut only = minimal_settings();
    only.items.volume = true;
    assert_eq!(
        keys(&DockEngine::new(only).snapshot()),
        vec!["fixed:volume"]
    );
}

#[test]
fn the_volume_title_says_the_level_muted_or_no_output() {
    for (reading, expected) in [
        (Some(level(45, false)), "Volume 45 %"),
        (Some(level(0, false)), "Volume 0 %"),
        (Some(level(100, false)), "Volume 100 %"),
        (Some(level(45, true)), "Volume muted"),
        (Some(level(0, true)), "Volume muted"),
        (None, "No audio output"),
    ] {
        assert_eq!(volume_title(reading.as_ref()), expected, "{reading:?}");
        let mut engine = DockEngine::new(with_volume(true));
        engine.apply(volume(reading));
        assert_eq!(
            item(&engine.snapshot(), "fixed:volume").map(|i| i.title.as_str()),
            Some(expected)
        );
    }
}

#[test]
fn switching_the_volume_item_bumps_the_revision_and_keeps_the_reading() {
    let mut settings = Settings::default();
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(volume(Some(level(30, false))));
    let r = engine.snapshot().revision;
    settings.items.volume = true;
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    assert_eq!(engine.snapshot().revision, r + 1);
    settings.items.volume = false;
    assert!(engine.apply(EngineInput::SettingsChanged(settings)).changed);
    assert_eq!(engine.volume(), Some(level(30, false)));
}

#[test]
fn a_click_on_the_volume_item_opens_quick_settings() {
    let mut engine = DockEngine::new(with_volume(true));
    for dock in [DOCK, "dock-DISPLAY2"] {
        assert_eq!(
            engine.primary_action("fixed:volume", dock),
            ItemAction::Shell(ShellAction::OpenQuickSettings)
        );
    }
    engine.apply(volume(Some(level(20, true))));
    assert_eq!(
        engine.primary_action("fixed:volume", DOCK),
        ItemAction::Shell(ShellAction::OpenQuickSettings)
    );
    assert_eq!(
        engine.primary_action("shell:volume", DOCK),
        ItemAction::None
    );
    // Its menu is built from the item as the dock lists it.
    let menu_item = engine.dock_item("fixed:volume", DOCK).unwrap();
    assert_eq!(menu_item.kind, DockItemKind::Volume);
    assert_eq!(menu_item.title, "Volume muted");
}

#[test]
fn a_scroll_over_the_volume_item_steps_the_volume_whatever_the_scroll_action() {
    use ScrollDirection::{Next, Previous};
    let mut settings = with_volume(true);
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(volume(Some(level(60, false))));
    // `none`, the default: the volume still steps, other items do not.
    assert_eq!(settings.behavior.scroll_action, ScrollAction::None);
    for direction in [Next, Previous] {
        assert_eq!(
            engine.scroll_action("fixed:volume", DOCK, direction, t0()),
            ScrollEffect::StepVolume(direction)
        );
        assert_eq!(
            engine.scroll_action("fixed:launcher", DOCK, direction, t0()),
            ScrollEffect::Nothing
        );
    }
    // `cycleWindows`: the same, on any dock window.
    settings = cycling(settings);
    engine.apply(EngineInput::SettingsChanged(settings.clone()));
    assert_eq!(
        engine.scroll_action("fixed:volume", "dock-DISPLAY2", Previous, t0()),
        ScrollEffect::StepVolume(Previous)
    );
    // Without a device the caller finds out when it reads the volume.
    engine.apply(volume(None));
    assert_eq!(
        engine.scroll_action("fixed:volume", DOCK, Next, t0()),
        ScrollEffect::StepVolume(Next)
    );
    // The item off: its key is unknown.
    settings.items.volume = false;
    engine.apply(EngineInput::SettingsChanged(settings));
    assert_eq!(
        engine.scroll_action("fixed:volume", DOCK, Next, t0()),
        ScrollEffect::UnknownItem
    );
}

#[test]
fn a_volume_step_moves_two_points_clamps_and_unmutes_going_up() {
    use ScrollDirection::{Next, Previous};
    assert_eq!(VOLUME_STEP, 2);
    for (from, direction, to) in [
        // Up: two points, unmuted, never past 100.
        (level(45, false), Previous, level(47, false)),
        (level(98, false), Previous, level(100, false)),
        (level(99, false), Previous, level(100, false)),
        (level(100, false), Previous, level(100, false)),
        (level(45, true), Previous, level(47, false)),
        (level(100, true), Previous, level(100, false)),
        (level(0, true), Previous, level(2, false)),
        // Down: two points, the mute kept, never below 0.
        (level(45, false), Next, level(43, false)),
        (level(2, false), Next, level(0, false)),
        (level(1, false), Next, level(0, false)),
        (level(0, false), Next, level(0, false)),
        (level(45, true), Next, level(43, true)),
        (level(0, true), Next, level(0, true)),
        // A reading out of range (never sent) counts as 100.
        (level(250, false), Next, level(98, false)),
        (level(250, false), Previous, level(100, false)),
    ] {
        assert_eq!(volume_step(from, direction), to, "{from:?} {direction:?}");
    }
    // Notches up from 0 reach 100 and stay there.
    let mut v = level(0, false);
    for _ in 0..60 {
        v = volume_step(v, Previous);
    }
    assert_eq!(v, level(100, false));
}
