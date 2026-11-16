use std::path::Path;

use super::*;

#[test]
fn user_css_serialises_in_camel_case_with_a_null_css() {
    let none = UserCss {
        path: r"C:\Users\me\AppData\Roaming\dev.davidcristian.stem\dock.css".to_owned(),
        css: None,
        problem: Some(UserCssProblem::NotFound),
    };
    let json = serde_json::to_value(&none).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "path": r"C:\Users\me\AppData\Roaming\dev.davidcristian.stem\dock.css",
            "css": null,
            "problem": "notFound",
        })
    );
    assert_eq!(serde_json::from_value::<UserCss>(json).unwrap(), none);
    let some = UserCss {
        css: Some(".dock-stage { --stem-dot: red; }".to_owned()),
        problem: None,
        ..none
    };
    let json = serde_json::to_value(&some).unwrap();
    assert_eq!(json["css"], ".dock-stage { --stem-dot: red; }");
    assert_eq!(json["problem"], serde_json::Value::Null);
    for (problem, name) in [
        (UserCssProblem::NotFound, "notFound"),
        (UserCssProblem::Unreadable, "unreadable"),
        (UserCssProblem::NotUtf8, "notUtf8"),
        (UserCssProblem::TooLarge, "tooLarge"),
    ] {
        assert_eq!(serde_json::to_value(problem).unwrap(), name);
    }
}

#[test]
fn scroll_direction_crosses_ipc_as_next_and_previous() {
    for (direction, name) in [
        (ScrollDirection::Next, "next"),
        (ScrollDirection::Previous, "previous"),
    ] {
        let json = serde_json::to_string(&direction).unwrap();
        assert_eq!(json, format!("{name:?}"));
        assert_eq!(
            serde_json::from_str::<ScrollDirection>(&json).unwrap(),
            direction
        );
    }
    assert!(serde_json::from_str::<ScrollDirection>(r#""up""#).is_err());
}

#[test]
fn app_id_for_exe_is_case_and_slash_insensitive() {
    let a = AppId::for_exe(Path::new(r"C:\Program Files\Foo\Foo.exe"));
    let b = AppId::for_exe(Path::new("c:/program files/foo/foo.exe"));
    assert_eq!(a, b);
    assert_eq!(a.as_str(), "exe:c:/program files/foo/foo.exe");
}

#[test]
fn launch_target_serialises_with_kind_tag() {
    let t = LaunchTarget::StoreApp {
        app_user_model_id: "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into(),
    };
    let json = serde_json::to_string(&t).unwrap();
    assert_eq!(
        json,
        r#"{"kind":"storeApp","appUserModelId":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"}"#
    );
    let back: LaunchTarget = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
}

#[test]
fn quick_settings_and_notification_center_serialise_in_camel_case() {
    for (kind, name) in [
        (DockItemKind::QuickSettings, "quickSettings"),
        (DockItemKind::NotificationCenter, "notificationCenter"),
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, format!("{name:?}"));
        assert_eq!(serde_json::from_str::<DockItemKind>(&json).unwrap(), kind);
    }
    for (action, name) in [
        (ShellAction::OpenQuickSettings, "openQuickSettings"),
        (
            ShellAction::OpenNotificationCenter,
            "openNotificationCenter",
        ),
    ] {
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, format!("{name:?}"));
        assert_eq!(serde_json::from_str::<ShellAction>(&json).unwrap(), action);
    }
}

#[test]
fn clock_battery_and_their_settings_pages_serialise_in_camel_case() {
    for (kind, name) in [
        (DockItemKind::Battery, "battery"),
        (DockItemKind::Clock, "clock"),
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, format!("{name:?}"));
        assert_eq!(serde_json::from_str::<DockItemKind>(&json).unwrap(), kind);
    }
    for (action, name) in [
        (ShellAction::OpenDateTimeSettings, "openDateTimeSettings"),
        (ShellAction::OpenPowerSettings, "openPowerSettings"),
    ] {
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, format!("{name:?}"));
        assert_eq!(serde_json::from_str::<ShellAction>(&json).unwrap(), action);
    }
}

#[test]
fn the_volume_item_its_status_and_its_settings_pages_serialise_in_camel_case() {
    let json = serde_json::to_string(&DockItemKind::Volume).unwrap();
    assert_eq!(json, r#""volume""#);
    assert_eq!(
        serde_json::from_str::<DockItemKind>(&json).unwrap(),
        DockItemKind::Volume
    );
    for (action, name) in [
        (ShellAction::OpenSoundSettings, "openSoundSettings"),
        (ShellAction::OpenVolumeMixer, "openVolumeMixer"),
    ] {
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, format!("{name:?}"));
        assert_eq!(serde_json::from_str::<ShellAction>(&json).unwrap(), action);
    }
    let status = VolumeStatus {
        percent: 45,
        muted: true,
    };
    let json = serde_json::to_value(status).unwrap();
    assert_eq!(json, serde_json::json!({ "percent": 45, "muted": true }));
    assert_eq!(
        serde_json::from_value::<VolumeStatus>(json).unwrap(),
        status
    );
    // In the snapshot under its own name, `null` without a device.
    let state = DockState {
        volume: Some(status),
        ..DockState::default()
    };
    assert_eq!(
        serde_json::to_value(&state).unwrap()["volume"],
        serde_json::json!({ "percent": 45, "muted": true })
    );
    assert_eq!(
        serde_json::to_value(DockState::default()).unwrap()["volume"],
        serde_json::Value::Null
    );
}

#[test]
fn dock_state_without_a_volume_still_deserialises() {
    // `volume` arrived with IPC v1.30; a document written before
    // it reads as the engine's "nothing known yet".
    let state: DockState = serde_json::from_str(
        r#"{"revision":5,"items":[],"foreground":null,"docks":[],
            "power":{"hasBattery":false,"percent":null,"charging":false,"onAc":false,"saver":false},
            "clockFormat":{"locale":null,"hour12":null}}"#,
    )
    .unwrap();
    assert_eq!(state.revision, 5);
    assert_eq!(state.volume, None);
}

#[test]
fn power_status_serialises_in_camel_case_with_a_null_percent() {
    let unknown = PowerStatus::default();
    assert_eq!(
        serde_json::to_value(unknown).unwrap(),
        serde_json::json!({
            "hasBattery": false,
            "percent": null,
            "charging": false,
            "onAc": false,
            "saver": false,
        })
    );
    let laptop = PowerStatus {
        has_battery: true,
        percent: Some(73),
        charging: true,
        on_ac: true,
        saver: false,
    };
    let json = serde_json::to_value(laptop).unwrap();
    assert_eq!(json["percent"], 73);
    assert_eq!(json["onAc"], true);
    assert_eq!(serde_json::from_value::<PowerStatus>(json).unwrap(), laptop);
}

#[test]
fn dock_state_without_power_still_deserialises() {
    // `power` arrived with the battery item; a document written before
    // it reads as the engine's "nothing known yet".
    let state: DockState =
        serde_json::from_str(r#"{"revision":3,"items":[],"foreground":null,"docks":[]}"#).unwrap();
    assert_eq!(state.revision, 3);
    assert_eq!(state.power, PowerStatus::default());
}

#[test]
fn clock_format_serialises_in_camel_case_with_nulls() {
    assert_eq!(
        serde_json::to_value(ClockFormat::default()).unwrap(),
        serde_json::json!({ "locale": null, "hour12": null })
    );
    let british = ClockFormat {
        locale: Some("en-GB".into()),
        hour12: Some(false),
    };
    let json = serde_json::to_value(&british).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "locale": "en-GB", "hour12": false })
    );
    assert_eq!(
        serde_json::from_value::<ClockFormat>(json).unwrap(),
        british
    );
    // In the snapshot under its camelCase name.
    let state = DockState {
        clock_format: british.clone(),
        ..DockState::default()
    };
    assert_eq!(
        serde_json::to_value(&state).unwrap()["clockFormat"],
        serde_json::json!({ "locale": "en-GB", "hour12": false })
    );
}

#[test]
fn dock_state_without_a_clock_format_still_deserialises() {
    // `clockFormat` arrived with IPC v1.25; a document written
    // before it reads as the engine's "nothing known yet".
    let state: DockState = serde_json::from_str(
        r#"{"revision":4,"items":[],"foreground":null,"docks":[],
            "power":{"hasBattery":false,"percent":null,"charging":false,"onAc":false,"saver":false}}"#,
    )
    .unwrap();
    assert_eq!(state.revision, 4);
    assert_eq!(state.clock_format, ClockFormat::default());
}

#[test]
fn raw_window_without_a_rect_or_desktop_still_deserialises() {
    // `rect` arrived with fullscreen detection and `onCurrentDesktop`
    // with virtual-desktop support; documents and mocks written before
    // them leave them out.
    let json = r#"{"id":7,"title":"T","className":"C","processId":1,"exePath":null,
        "appUserModelId":null,"isVisible":true,"isMinimized":false,"isCloaked":false,
        "monitor":null,"iconKey":null}"#;
    let raw: RawWindow = serde_json::from_str(json).unwrap();
    assert_eq!(raw.rect, None);
    assert_eq!(raw.on_current_desktop, None);
    let with_rect: RawWindow = serde_json::from_str(&json.replacen(
        r#""monitor":null"#,
        r#""monitor":null,"rect":{"x":1,"y":2,"width":3,"height":4}"#,
        1,
    ))
    .unwrap();
    assert_eq!(with_rect.rect, Some(Rect::new(1, 2, 3, 4)));
    for (value, expected) in [("true", Some(true)), ("false", Some(false)), ("null", None)] {
        let with_desktop: RawWindow = serde_json::from_str(&json.replacen(
            r#""monitor":null"#,
            &format!(r#""monitor":null,"onCurrentDesktop":{value}"#),
            1,
        ))
        .unwrap();
        assert_eq!(with_desktop.on_current_desktop, expected, "{value}");
    }
}

#[test]
fn app_info_and_app_folder_serialise_in_camel_case() {
    let info = AppInfo {
        version: "0.1.0".to_owned(),
        settings_path: r"C:\Users\me\AppData\Roaming\dev.davidcristian.stem\settings.json"
            .to_owned(),
        log_path: String::new(),
        platform: "windows".to_owned(),
    };
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "version": "0.1.0",
            "settingsPath": r"C:\Users\me\AppData\Roaming\dev.davidcristian.stem\settings.json",
            "logPath": "",
            "platform": "windows",
        })
    );
    assert_eq!(serde_json::from_value::<AppInfo>(json).unwrap(), info);
    assert_eq!(
        serde_json::to_string(&AppFolder::Logs).unwrap(),
        r#""logs""#
    );
    assert_eq!(
        serde_json::from_str::<AppFolder>(r#""settings""#).unwrap(),
        AppFolder::Settings
    );
}

#[test]
fn rect_contains_is_half_open() {
    let r = Rect::new(10, 10, 5, 5);
    assert!(r.contains(10, 10));
    assert!(r.contains(14, 14));
    assert!(!r.contains(15, 14));
    assert!(!r.contains(9, 10));
}
