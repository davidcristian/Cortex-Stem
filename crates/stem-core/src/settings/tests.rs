use std::path::PathBuf;

use pretty_assertions::assert_eq;

use super::*;
use crate::model::{AppId, IconKey, LaunchTarget};

fn pin(id: &str) -> PinnedApp {
    PinnedApp {
        id: AppId(id.to_owned()),
        label: None,
        launch: LaunchTarget::Exe {
            path: PathBuf::from(id),
            args: vec![],
            working_dir: None,
        },
    }
}

/// Tiny deterministic generator (xorshift64*) so the property-style tests
/// need no extra dependency and reproduce exactly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn bool(&mut self) -> bool {
        self.below(2) == 1
    }

    /// A `u32` that is often an edge value.
    fn u32(&mut self) -> u32 {
        match self.below(6) {
            0 => 0,
            1 => u32::MAX,
            2 => 1,
            _ => self.next() as u32 % 10_000,
        }
    }

    /// An `f32` that is often an edge value, including non-finite ones.
    fn f32(&mut self) -> f32 {
        match self.below(8) {
            0 => f32::NAN,
            1 => f32::INFINITY,
            2 => f32::NEG_INFINITY,
            3 => -1.0,
            4 => 0.0,
            5 => 1e9,
            _ => (self.below(10_000) as f32) / 1_000.0,
        }
    }

    fn pick<T: Copy>(&mut self, options: &[T]) -> T {
        options[self.below(options.len() as u64) as usize]
    }
}

fn arbitrary(rng: &mut Rng) -> Settings {
    Settings {
        version: rng.below(2) as u32,
        position: rng.pick(&[
            DockPosition::Bottom,
            DockPosition::Left,
            DockPosition::Right,
            DockPosition::Top,
        ]),
        icon_size: rng.u32(),
        magnification: Magnification {
            enabled: rng.bool(),
            max_scale: rng.f32(),
            range_icons: rng.u32(),
        },
        auto_hide: AutoHide {
            mode: rng.pick(&[
                AutoHideMode::Off,
                AutoHideMode::Always,
                AutoHideMode::WhenOverlapped,
            ]),
            delay_ms: rng.u32(),
        },
        monitors: rng.pick(&[MonitorMode::Primary, MonitorMode::All]),
        theme: rng.pick(&[ThemeMode::System, ThemeMode::Light, ThemeMode::Dark]),
        appearance: Appearance {
            theme: rng.pick(&[
                ThemePreset::Standard,
                ThemePreset::Solid,
                ThemePreset::Minimal,
            ]),
            opacity: rng.f32(),
            corner_radius: rng.u32(),
            backdrop: rng.pick(&[Backdrop::Css, Backdrop::Acrylic, Backdrop::Mica]),
            tooltips: rng.bool(),
            bounce_on_launch: rng.bool(),
        },
        items: FixedItems {
            launcher: rng.bool(),
            task_view: rng.bool(),
            show_desktop: rng.bool(),
            quick_settings: rng.bool(),
            notification_center: rng.bool(),
            volume: rng.bool(),
            battery: rng.bool(),
            clock: rng.bool(),
            recycle_bin: rng.bool(),
            separators: rng.bool(),
        },
        behavior: Behavior {
            click_active_minimizes: rng.bool(),
            show_only_current_desktop: rng.bool(),
            show_only_current_monitor: rng.bool(),
            running_indicators: rng.bool(),
            click_through: rng.bool(),
            toggle_shortcut: rng
                .pick(&["", " ", DEFAULT_TOGGLE_SHORTCUT, "ctrl+shift+F9", "Nope+?"])
                .to_owned(),
            focus_shortcut: rng
                .pick(&[
                    "",
                    " ",
                    DEFAULT_FOCUS_SHORTCUT,
                    DEFAULT_TOGGLE_SHORTCUT,
                    "alt+ctrl+d",
                    "Nope+?",
                ])
                .to_owned(),
            scroll_action: rng.pick(&[ScrollAction::None, ScrollAction::CycleWindows]),
        },
        pinned: (0..rng.below(8))
            .map(|_| {
                let id = rng.pick(&["", " ", "a", "b", "c", "exe:c:/x.exe", "aumid:X!App"]);
                let mut p = pin(id);
                p.label = rng.bool().then(|| "Label".to_owned());
                p
            })
            .collect(),
        // The same small id pool as the pins, so hidden ids that are
        // also pinned, blank or repeated come up often.
        hidden_apps: (0..rng.below(6))
            .map(|_| HiddenApp {
                id: AppId(
                    rng.pick(&["", " ", "a", "b", "d", "exe:c:/x.exe", "aumid:Y!App"])
                        .to_owned(),
                ),
                name: rng.pick(&["", "Name", "Other"]).to_owned(),
                path: rng.bool().then(|| r"C:\Program Files\X\X.exe".to_owned()),
                icon_key: rng.bool().then(|| IconKey("exe:C:\\x.exe".to_owned())),
            })
            .collect(),
        system: SystemIntegration {
            autostart: rng.bool(),
            hide_windows_taskbar: rng.bool(),
            reserve_screen_space: rng.bool(),
            file_logging: rng.bool(),
        },
    }
}

fn assert_within_ranges(s: &Settings) {
    assert!((24..=128).contains(&s.icon_size));
    assert!((1.0..=2.5).contains(&s.magnification.max_scale));
    assert!((1..=6).contains(&s.magnification.range_icons));
    assert!(s.auto_hide.delay_ms <= 5_000);
    assert!((0.0..=1.0).contains(&s.appearance.opacity));
    assert!(s.appearance.corner_radius <= 64);
    let ids: Vec<&str> = s.pinned.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.iter().all(|id| !id.trim().is_empty()));
    let mut dedup = ids.clone();
    dedup.sort_unstable();
    dedup.dedup();
    assert_eq!(dedup.len(), ids.len(), "duplicate pin ids in {ids:?}");
    let hidden: Vec<&str> = s.hidden_apps.iter().map(|h| h.id.as_str()).collect();
    assert!(hidden.iter().all(|id| !id.trim().is_empty()));
    assert!(
        hidden.iter().all(|id| !ids.contains(id)),
        "pinned ids {ids:?} hidden in {hidden:?}"
    );
    let mut dedup = hidden.clone();
    dedup.sort_unstable();
    dedup.dedup();
    assert_eq!(
        dedup.len(),
        hidden.len(),
        "duplicate hidden ids in {hidden:?}"
    );
}

#[test]
fn empty_document_yields_defaults() {
    let s = Settings::from_json("{}").unwrap();
    assert_eq!(s, Settings::default());
}

#[test]
fn a_byte_order_mark_is_ignored() {
    let s = Settings::from_json("\u{feff}{\"iconSize\": 64}").unwrap();
    assert_eq!(s.icon_size, 64);
    // Only a leading one: inside the document it is still an error.
    assert!(Settings::from_json("{\u{feff}\"iconSize\": 64}").is_err());
}

#[test]
fn unknown_fields_are_ignored_and_partial_structs_fill_in() {
    let s =
        Settings::from_json(r#"{"iconSize": 64, "magnification": {"maxScale": 2.0}, "bogus": 1}"#)
            .unwrap();
    assert_eq!(s.icon_size, 64);
    assert_eq!(s.magnification.max_scale, 2.0);
    assert!(s.magnification.enabled);
}

#[test]
fn toggle_shortcut_defaults_when_missing_and_keeps_what_is_written() {
    assert_eq!(Behavior::default().toggle_shortcut, "Ctrl+Alt+D");

    // A file written before the field existed (here a v0 file, without
    // a version either) gets the default next to its other values.
    let old = Settings::from_json(r#"{"behavior": {"clickActiveMinimizes": false}}"#).unwrap();
    assert!(!old.behavior.click_active_minimizes);
    assert_eq!(old.behavior.toggle_shortcut, DEFAULT_TOGGLE_SHORTCUT);
    assert_eq!(old.version, SETTINGS_VERSION);

    // Empty (no shortcut) and values the app cannot register are kept
    // as written, so the user sees and can fix them.
    for written in ["", "Shift+Alt+F12", "not a shortcut"] {
        let json = format!(r#"{{"behavior": {{"toggleShortcut": {written:?}}}}}"#);
        let s = Settings::from_json(&json).unwrap();
        assert_eq!(s.behavior.toggle_shortcut, written);
        assert!(s.to_json_pretty().contains(r#""toggleShortcut""#));
        assert_eq!(Settings::from_json(&s.to_json_pretty()).unwrap(), s);
    }
}

#[test]
fn focus_shortcut_defaults_when_missing_and_keeps_what_is_written() {
    assert_eq!(Behavior::default().focus_shortcut, "Ctrl+Alt+Shift+D");
    assert_ne!(DEFAULT_FOCUS_SHORTCUT, DEFAULT_TOGGLE_SHORTCUT);

    // A file written before the field existed gets the default, and the
    // toggle shortcut it has is left alone.
    let old =
        Settings::from_json(r#"{"version": 1, "behavior": {"toggleShortcut": "F9"}}"#).unwrap();
    assert_eq!(old.behavior.toggle_shortcut, "F9");
    assert_eq!(old.behavior.focus_shortcut, DEFAULT_FOCUS_SHORTCUT);

    // Empty (no shortcut), the toggle shortcut's own chord and values
    // the app cannot register are kept as written: the app layer
    // decides what registers, so the user sees and can fix the value.
    for written in ["", "ctrl+alt+d", "Shift+Alt+F12", "not a shortcut"] {
        let json = format!(r#"{{"behavior": {{"focusShortcut": {written:?}}}}}"#);
        let s = Settings::from_json(&json).unwrap();
        assert_eq!(s.behavior.focus_shortcut, written);
        assert_eq!(s.behavior.toggle_shortcut, DEFAULT_TOGGLE_SHORTCUT);
        assert!(s.to_json_pretty().contains(r#""focusShortcut""#));
        assert_eq!(Settings::from_json(&s.to_json_pretty()).unwrap(), s);
    }
}

#[test]
fn scroll_action_defaults_to_none_and_round_trips_in_camel_case() {
    assert_eq!(Behavior::default().scroll_action, ScrollAction::None);

    // A file written before the field existed gets the default next to
    // its other behaviour values.
    let old = Settings::from_json(r#"{"behavior": {"clickActiveMinimizes": false}}"#).unwrap();
    assert!(!old.behavior.click_active_minimizes);
    assert_eq!(old.behavior.scroll_action, ScrollAction::None);

    for (action, name) in [
        (ScrollAction::None, "none"),
        (ScrollAction::CycleWindows, "cycleWindows"),
    ] {
        assert_eq!(serde_json::to_string(&action).unwrap(), format!("{name:?}"));
        let json = format!(r#"{{"behavior": {{"scrollAction": {name:?}}}}}"#);
        let s = Settings::from_json(&json).unwrap();
        assert_eq!(s.behavior.scroll_action, action);
        assert!(
            s.to_json_pretty()
                .contains(&format!(r#""scrollAction": {name:?}"#))
        );
        assert_eq!(Settings::from_json(&s.to_json_pretty()).unwrap(), s);
    }

    // Like every other enum value: a name Stem does not know makes the
    // file unusable rather than silently `none`.
    assert!(matches!(
        Settings::from_json(r#"{"behavior": {"scrollAction": "cycle"}}"#),
        Err(SettingsError::Parse(_))
    ));
}

#[test]
fn theme_preset_defaults_to_standard_and_round_trips_in_camel_case() {
    assert_eq!(Appearance::default().theme, ThemePreset::Standard);

    // A file written before the field existed gets the default next to
    // its other appearance values.
    let old = Settings::from_json(r#"{"appearance": {"opacity": 0.5}}"#).unwrap();
    assert_eq!(old.appearance.theme, ThemePreset::Standard);
    assert_eq!(old.appearance.opacity, 0.5);

    for (preset, name) in [
        (ThemePreset::Standard, "standard"),
        (ThemePreset::Solid, "solid"),
        (ThemePreset::Minimal, "minimal"),
    ] {
        assert_eq!(serde_json::to_string(&preset).unwrap(), format!("{name:?}"));
        let json = format!(r#"{{"appearance": {{"theme": {name:?}}}}}"#);
        let s = Settings::from_json(&json).unwrap();
        assert_eq!(s.appearance.theme, preset);
        assert!(
            s.to_json_pretty()
                .contains(&format!(r#""theme": {name:?}"#))
        );
        assert_eq!(Settings::from_json(&s.to_json_pretty()).unwrap(), s);
    }

    // Like every other enum value: a name Stem does not know makes the
    // file unusable rather than silently standard (the app keeps it as
    // `settings.json.broken`).
    assert!(matches!(
        Settings::from_json(r#"{"appearance": {"theme": "glass"}}"#),
        Err(SettingsError::Parse(_))
    ));
}

#[test]
fn quick_settings_and_notification_center_are_off_unless_written() {
    let items = FixedItems::default();
    assert!(!items.quick_settings);
    assert!(!items.notification_center);

    // A file written before the fields existed keeps its other items and
    // gets both off.
    let old = Settings::from_json(r#"{"items": {"launcher": false, "taskView": true}}"#).unwrap();
    assert!(!old.items.launcher);
    assert!(old.items.task_view);
    assert!(!old.items.quick_settings);
    assert!(!old.items.notification_center);

    // Both are camelCase keys of `items` and survive a round trip.
    let on =
        Settings::from_json(r#"{"items": {"quickSettings": true, "notificationCenter": true}}"#)
            .unwrap();
    assert!(on.items.quick_settings);
    assert!(on.items.notification_center);
    let json = on.to_json_pretty();
    assert!(json.contains(r#""quickSettings": true"#), "{json}");
    assert!(json.contains(r#""notificationCenter": true"#), "{json}");
    assert_eq!(Settings::from_json(&json).unwrap(), on);
}

#[test]
fn clock_and_battery_are_off_unless_written() {
    let items = FixedItems::default();
    assert!(!items.clock);
    assert!(!items.battery);

    // A file written before the fields existed keeps its other
    // items and gets both off.
    let old = Settings::from_json(
        r#"{"items": {"launcher": false, "quickSettings": true, "recycleBin": false}}"#,
    )
    .unwrap();
    assert!(!old.items.launcher);
    assert!(old.items.quick_settings);
    assert!(!old.items.recycle_bin);
    assert!(!old.items.clock);
    assert!(!old.items.battery);

    // Both are camelCase keys of `items`, independent of each other, and
    // survive a round trip.
    for (clock, battery) in [(true, false), (false, true), (true, true)] {
        let on = Settings::from_json(&format!(
            r#"{{"items": {{"clock": {clock}, "battery": {battery}}}}}"#
        ))
        .unwrap();
        assert_eq!((on.items.clock, on.items.battery), (clock, battery));
        let json = on.to_json_pretty();
        assert!(json.contains(&format!(r#""clock": {clock}"#)), "{json}");
        assert!(json.contains(&format!(r#""battery": {battery}"#)), "{json}");
        assert_eq!(Settings::from_json(&json).unwrap(), on);
        // `validate` leaves them as written.
        let mut validated = on.clone();
        validated.validate();
        assert_eq!(validated, on);
    }
}

#[test]
fn the_volume_item_is_off_unless_written() {
    assert!(!FixedItems::default().volume);
    // A file written before the field existed (IPC v1.30) keeps
    // its other items and gets it off.
    let old = Settings::from_json(r#"{"items": {"battery": true, "clock": true}}"#).unwrap();
    assert!(old.items.battery && old.items.clock);
    assert!(!old.items.volume);
    // A camelCase key of `items`, independent of the others, that
    // survives a round trip and `validate`.
    let on = Settings::from_json(r#"{"items": {"volume": true}}"#).unwrap();
    assert!(on.items.volume);
    assert!(!on.items.battery && !on.items.clock);
    let json = on.to_json_pretty();
    assert!(json.contains(r#""volume": true"#), "{json}");
    assert_eq!(Settings::from_json(&json).unwrap(), on);
    let mut validated = on.clone();
    validated.validate();
    assert_eq!(validated, on);
}

#[test]
fn validate_clamps_and_dedupes() {
    let mut s = Settings {
        icon_size: 999,
        ..Default::default()
    };
    s.magnification.max_scale = 9.0;
    s.pinned = vec![pin("a"), pin("a"), pin(""), pin("b")];
    s.validate();
    assert_eq!(s.icon_size, 128);
    assert_eq!(s.magnification.max_scale, 2.5);
    assert_eq!(
        s.pinned.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
}

#[test]
fn validate_drops_blank_pin_ids_and_keeps_the_first_duplicate() {
    let first = PinnedApp {
        label: Some("first".into()),
        ..pin("a")
    };
    let second = PinnedApp {
        label: Some("second".into()),
        ..pin("a")
    };
    let mut s = Settings {
        pinned: vec![pin(" \t"), first, pin("b"), second],
        ..Default::default()
    };
    s.validate();
    assert_eq!(
        s.pinned.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(s.pinned[0].label.as_deref(), Some("first"));
}

fn hidden(id: &str, name: &str) -> HiddenApp {
    HiddenApp {
        id: AppId(id.to_owned()),
        name: name.to_owned(),
        path: None,
        icon_key: None,
    }
}

fn hidden_ids(s: &Settings) -> Vec<&str> {
    s.hidden_apps.iter().map(|h| h.id.as_str()).collect()
}

#[test]
fn hidden_apps_are_empty_unless_written_and_round_trip() {
    assert!(Settings::default().hidden_apps.is_empty());

    // A file written before the field existed keeps its pins and hides
    // nothing.
    let old = Settings::from_json(
        r#"{"pinned": [{"id": "exe:c:/a.exe", "launch": {"kind": "uri", "uri": "a:"}}]}"#,
    )
    .unwrap();
    assert_eq!(old.pinned.len(), 1);
    assert!(old.hidden_apps.is_empty());

    // camelCase key, `{ id, name }` entries; a hand-written entry without
    // a name loads with an empty one, one without an id does not load.
    let s = Settings::from_json(
        r#"{"hiddenApps": [{"id": "exe:c:/b.exe", "name": "B"}, {"id": "aumid:C!App"}]}"#,
    )
    .unwrap();
    assert_eq!(
        s.hidden_apps,
        vec![hidden("exe:c:/b.exe", "B"), hidden("aumid:C!App", "")]
    );
    let json = s.to_json_pretty();
    assert!(json.contains(r#""hiddenApps""#), "{json}");
    assert!(json.contains(r#""name": "B""#), "{json}");
    assert_eq!(Settings::from_json(&json).unwrap(), s);
    assert!(matches!(
        Settings::from_json(r#"{"hiddenApps": [{"name": "B"}]}"#),
        Err(SettingsError::Parse(_))
    ));
}

#[test]
fn a_hidden_app_keeps_its_path_and_icon_key_and_old_entries_load_without_them() {
    // An older entry: no path, no icon key.
    let old =
        Settings::from_json(r#"{"hiddenApps": [{"id": "exe:c:/b.exe", "name": "B"}]}"#).unwrap();
    assert_eq!(old.hidden_apps, vec![hidden("exe:c:/b.exe", "B")]);

    let full = HiddenApp {
        path: Some(r"C:\Program Files\B\B.exe".to_owned()),
        icon_key: Some(IconKey(r"exe:C:\Program Files\B\B.exe".to_owned())),
        ..hidden("exe:c:/program files/b/b.exe", "B")
    };
    let mut s = Settings {
        hidden_apps: vec![full.clone()],
        ..Default::default()
    };
    s.validate();
    assert_eq!(s.hidden_apps, vec![full.clone()], "validate keeps both");
    let json = s.to_json_pretty();
    assert!(
        json.contains(r#""iconKey": "exe:C:\\Program Files\\B\\B.exe""#),
        "{json}"
    );
    assert!(
        json.contains(r#""path": "C:\\Program Files\\B\\B.exe""#),
        "{json}"
    );
    assert_eq!(Settings::from_json(&json).unwrap(), s);
    // `null` written out reads back as unknown.
    let unknown = HiddenApp {
        path: None,
        icon_key: None,
        ..full
    };
    let json = serde_json::to_value(&unknown).unwrap();
    assert_eq!(json["path"], serde_json::Value::Null);
    assert_eq!(json["iconKey"], serde_json::Value::Null);
    assert_eq!(serde_json::from_value::<HiddenApp>(json).unwrap(), unknown);
}

#[test]
fn validate_drops_pinned_blank_and_repeated_hidden_apps() {
    let mut s = Settings {
        pinned: vec![pin("a"), pin("a"), pin("b")],
        hidden_apps: vec![
            hidden("c", "first"),
            hidden("a", "pinned"),
            hidden(" \t", "blank"),
            hidden("", "empty"),
            hidden("d", "D"),
            hidden("c", "second"),
            hidden("b", "pinned too"),
        ],
        ..Default::default()
    };
    s.validate();
    assert_eq!(hidden_ids(&s), vec!["c", "d"]);
    assert_eq!(s.hidden_apps[0].name, "first");
    // A name is kept as written, empty included.
    let mut s = Settings {
        hidden_apps: vec![hidden("e", "")],
        ..Default::default()
    };
    s.validate();
    assert_eq!(s.hidden_apps, vec![hidden("e", "")]);
}

#[test]
fn hide_and_unhide_never_touch_a_pin() {
    let mut s = Settings {
        pinned: vec![pin("a")],
        ..Default::default()
    };
    assert!(!s.hide(hidden("a", "A")), "a pinned app is never hidden");
    assert!(s.hidden_apps.is_empty());
    assert!(s.hide(hidden("b", "B")));
    assert!(!s.hide(hidden("b", "B again")), "already hidden");
    assert!(s.is_hidden(&AppId("b".into())));
    assert!(!s.is_hidden(&AppId("a".into())));
    assert_eq!(s.hidden_apps, vec![hidden("b", "B")]);
    assert!(s.unhide(&AppId("b".into())));
    assert!(!s.unhide(&AppId("b".into())), "not hidden any more");
    assert!(!s.unhide(&AppId("a".into())));
    assert!(s.hidden_apps.is_empty());
    assert_eq!(s.pinned, vec![pin("a")]);

    // Pinning a hidden app and validating shows it again as a pin.
    assert!(s.hide(hidden("c", "C")));
    assert!(s.pin(pin("c")));
    s.validate();
    assert!(!s.is_hidden(&AppId("c".into())));
    assert!(s.is_pinned(&AppId("c".into())));
}

#[test]
fn validate_replaces_non_finite_floats_with_defaults() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut s = Settings::default();
        s.magnification.max_scale = bad;
        s.appearance.opacity = bad;
        s.validate();
        assert_eq!(
            s.magnification.max_scale,
            Magnification::default().max_scale
        );
        assert_eq!(s.appearance.opacity, Appearance::default().opacity);
        // And the document stays serialisable (JSON has no NaN/inf).
        assert_ne!(s.to_json_pretty(), "{}");
        assert_eq!(Settings::from_json(&s.to_json_pretty()).unwrap(), s);
    }
}

#[test]
fn validate_is_idempotent_for_default_and_example() {
    for mut s in [Settings::default(), Settings::example()] {
        let original = s.clone();
        s.validate();
        assert_eq!(s, original, "already-valid documents must not change");
        s.validate();
        assert_eq!(s, original);
    }
}

#[test]
fn validate_is_idempotent_on_pseudo_random_documents() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for i in 0..2_000 {
        let raw = arbitrary(&mut rng);
        let mut once = raw.clone();
        once.validate();
        assert_within_ranges(&once);
        let mut twice = once.clone();
        twice.validate();
        assert_eq!(once, twice, "iteration {i}: validate is not idempotent");

        // A validated document round-trips through JSON unchanged
        // (modulo the version stamp `from_json` applies).
        let back = Settings::from_json(&once.to_json_pretty())
            .unwrap_or_else(|e| panic!("iteration {i}: {e}"));
        let mut expected = once.clone();
        expected.migrate();
        assert_eq!(
            back, expected,
            "iteration {i}: round trip changed the document"
        );
    }
}

#[test]
fn diff_requires_window_rebuild_only_for_position_and_monitors() {
    let base = Settings::default();
    assert!(!base.diff_requires_window_rebuild(&base));

    let mut restyled = base.clone();
    restyled.icon_size = 96;
    restyled.appearance.opacity = 0.3;
    restyled.auto_hide.mode = AutoHideMode::Always;
    restyled.theme = ThemeMode::Dark;
    restyled.appearance.theme = ThemePreset::Minimal;
    restyled.items.launcher = false;
    restyled.pinned.push(pin("a"));
    restyled.hidden_apps.push(hidden("b", "B"));
    assert!(!base.diff_requires_window_rebuild(&restyled));

    let mut moved = base.clone();
    moved.position = DockPosition::Left;
    assert!(base.diff_requires_window_rebuild(&moved));
    assert!(moved.diff_requires_window_rebuild(&base));

    let mut everywhere = base.clone();
    everywhere.monitors = MonitorMode::All;
    assert!(base.diff_requires_window_rebuild(&everywhere));
}

#[test]
fn example_is_valid_pins_every_launch_kind_and_round_trips() {
    let example = Settings::example();
    assert_eq!(example.version, SETTINGS_VERSION);
    assert!((4..=5).contains(&example.pinned.len()));
    let kinds: Vec<&str> = example
        .pinned
        .iter()
        .map(|p| match p.launch {
            LaunchTarget::Exe { .. } => "exe",
            LaunchTarget::Shortcut { .. } => "shortcut",
            LaunchTarget::StoreApp { .. } => "storeApp",
            LaunchTarget::Uri { .. } => "uri",
        })
        .collect();
    for kind in ["exe", "shortcut", "storeApp", "uri"] {
        assert!(
            kinds.contains(&kind),
            "example has no {kind} pin: {kinds:?}"
        );
    }
    // Ids use the canonical forms the engine derives for running windows.
    assert!(
        example
            .pinned
            .iter()
            .all(|p| { p.id.as_str().starts_with("exe:") || p.id.as_str().starts_with("aumid:") })
    );
    assert!(example.is_pinned(&AppId::for_app_user_model_id(
        "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
    )));

    let json = example.to_json_pretty();
    assert!(json.contains(r#""kind": "shortcut""#));
    assert_eq!(Settings::from_json(&json).unwrap(), example);
    assert_ne!(example, Settings::default());
}

#[test]
fn newer_version_is_rejected() {
    let err = Settings::from_json(r#"{"version": 99}"#).unwrap_err();
    assert!(matches!(err, SettingsError::TooNew { found: 99, .. }));
}

#[test]
fn reorder_keeps_unlisted_pins_at_the_end() {
    let mut s = Settings {
        pinned: vec![pin("a"), pin("b"), pin("c"), pin("d")],
        ..Default::default()
    };
    s.reorder_pinned(&[AppId("c".into()), AppId("zzz".into()), AppId("a".into())]);
    assert_eq!(
        s.pinned.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        vec!["c", "a", "b", "d"]
    );
}

#[test]
fn round_trips_through_json() {
    let mut s = Settings::default();
    s.pinned.push(pin("exe:c:/x.exe"));
    let back = Settings::from_json(&s.to_json_pretty()).unwrap();
    assert_eq!(back, s);
}
