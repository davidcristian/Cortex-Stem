//! Hidden apps: hiding, showing again, and what a hidden app keeps.

use pretty_assertions::assert_eq;

use super::*;
use crate::hidden_apps::hidden_app_facts;

#[test]
fn a_hidden_app_is_saved_like_a_pin_loses_its_item_and_stays_hidden_after_a_restart() {
    let platform = Arc::new(MockPlatform::with_demo_scenario());
    let store = SharedStore::default();
    let document = Arc::clone(&store.0);
    let service = DockService::start(platform.clone(), Box::new(store)).unwrap();
    let rx = record(&service);
    let state = wait_state(&rx, resolved);
    let spotify = item_titled(&state, "Spotify").unwrap().clone();
    let spotify_id = spotify.app_id.clone().unwrap();

    service.hide_app(&spotify_id).unwrap();
    // The authoritative copy and the file both have it, with its name,
    // its program's path as the platform reported it and its icon.
    let settings = service.settings();
    assert_eq!(hidden_ids(&settings), vec![spotify_id.as_str()]);
    assert_eq!(settings.hidden_apps[0].name, "Spotify");
    let exe = platform.windows().unwrap()[5].exe_path.clone().unwrap();
    assert_eq!(
        settings.hidden_apps[0].path.as_deref(),
        Some(exe.to_string_lossy().as_ref())
    );
    assert_ne!(
        settings.hidden_apps[0].path.as_deref(),
        Some(&spotify_id.as_str()["exe:".len()..]),
        "the original case, not the id's"
    );
    assert!(spotify.icon_key.is_some());
    assert_eq!(settings.hidden_apps[0].icon_key, spotify.icon_key);
    let saved = Settings::from_json(&document.document().unwrap()).unwrap();
    assert_eq!(saved, settings);
    // The page hears of it as of any settings change, then of the state.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match rx.recv_deadline(deadline) {
            Ok(ServiceEvent::Settings(s)) if s.is_hidden(&spotify_id) => break,
            Ok(_) => {}
            Err(e) => panic!("no settings event: {e}"),
        }
    }
    let state = wait_state(&rx, |s| item_titled(s, "Spotify").is_none());
    assert_eq!(app_items(&state).len(), 4);

    // Its window is still tracked: a click on its old key does nothing,
    // and the platform is not asked to do anything.
    assert_eq!(
        service.windows_on_dock(&spotify_id, DOCK),
        vec![WindowId(6)]
    );
    platform.clear_calls();
    assert!(matches!(
        service.primary_click(&spotify.key, DOCK),
        Err(ServiceError::UnknownItem(_))
    ));
    assert_eq!(platform.calls(), vec![]);
    assert_eq!(service.dock_item(&spotify.key, DOCK), None);

    // A second window of it brings no item either.
    let mut second = platform.windows().unwrap()[5].clone();
    assert_eq!(second.id, WindowId(6));
    second.id = WindowId(60);
    second.title = "Spotify mini player".into();
    platform.push(PlatformEvent::WindowAdded(second));
    wait_until("the second window", || {
        service.windows_on_dock(&spotify_id, DOCK) == [WindowId(6), WindowId(60)]
    });
    assert!(item_titled(&service.state(), "Spotify").is_none());
    assert_eq!(service.state().revision, state.revision, "nothing to show");

    // Hiding it again saves nothing.
    let before = document.document();
    service.hide_app(&spotify_id).unwrap();
    assert_eq!(document.document(), before);
    assert_eq!(hidden_ids(&service.settings()), vec![spotify_id.as_str()]);
    service.shutdown();

    // A restart reads the list back: still no item.
    let restarted = DockService::start(
        Arc::new(MockPlatform::with_demo_scenario()),
        Box::new(MemorySettingsStore::with_document(&before.unwrap())),
    )
    .unwrap();
    let rx = record(&restarted);
    assert!(restarted.settings().is_hidden(&spotify_id));
    let state = wait_state(&rx, |s| {
        app_items(s).len() == 4 && app_items(s).iter().all(|i| i.icon_key.is_some())
    });
    assert!(item_titled(&state, "Spotify").is_none());

    // Shown again: the item comes back; showing it twice is no error.
    restarted.unhide_app(&spotify_id).unwrap();
    assert!(restarted.settings().hidden_apps.is_empty());
    wait_state(&rx, |s| item_titled(s, "Spotify").is_some());
    restarted.unhide_app(&spotify_id).unwrap();
    restarted.shutdown();
}

#[test]
fn a_pinned_or_unknown_app_cannot_be_hidden_and_a_pin_shows_a_hidden_app() {
    let platform = Arc::new(MockPlatform::with_demo_scenario());
    let store = SharedStore::default();
    let document = Arc::clone(&store.0);
    let service = DockService::start(platform, Box::new(store)).unwrap();
    let rx = record(&service);
    let state = wait_state(&rx, resolved);
    let code = item_titled(&state, "Code").unwrap().app_id.clone().unwrap();

    // Pinned (in the authoritative copy, before the engine has it).
    service.pin_app(&code).unwrap();
    let before = document.document();
    let err = service.hide_app(&code).unwrap_err();
    assert!(matches!(&err, ServiceError::Pinned(id) if *id == code));
    assert!(err.to_string().contains("is pinned"), "{err}");
    assert_eq!(document.document(), before, "nothing saved");
    assert!(service.settings().hidden_apps.is_empty());

    // Unknown: no window, no identity.
    let nobody = AppId::for_exe(Path::new(r"C:\Apps\nobody.exe"));
    assert!(matches!(
        service.hide_app(&nobody),
        Err(ServiceError::UnknownApp(id)) if id == nobody
    ));
    assert_eq!(document.document(), before);
    // Showing an app that is not hidden is no error and saves nothing.
    service.unhide_app(&nobody).unwrap();
    assert_eq!(document.document(), before);

    // Pinning a hidden app drops it from the list (validation), so it
    // is shown as a pin.
    let terminal = item_titled(&state, "WindowsTerminal")
        .unwrap()
        .app_id
        .clone()
        .unwrap();
    service.hide_app(&terminal).unwrap();
    assert!(service.settings().is_hidden(&terminal));
    service.pin_app(&terminal).unwrap();
    let settings = service.settings();
    assert!(settings.is_pinned(&terminal));
    assert!(settings.hidden_apps.is_empty());
    wait_state(&rx, |s| {
        app_items(s)
            .iter()
            .any(|i| i.app_id.as_ref() == Some(&terminal) && i.pinned)
    });
    service.shutdown();
}

/// Visual Studio Code's exe, in the case Windows reports it.
const CODE_EXE: &str = r"C:\Users\Owner\AppData\Local\Programs\Microsoft VS Code\Code.exe";
/// Its Start-menu shortcut.
const CODE_LNK: &str = r"C:\Users\Owner\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Visual Studio Code\Visual Studio Code.lnk";

/// A running Code window: no AppUserModelID of its own, so an `exe:` id.
fn code_window() -> RawWindow {
    RawWindow {
        id: WindowId(50),
        title: "lib.rs - Stem".into(),
        class_name: "Chrome_WidgetWin_1".into(),
        process_id: 5050,
        exe_path: Some(PathBuf::from(CODE_EXE)),
        app_user_model_id: None,
        icon_key: None,
        ..terminal_window()
    }
}

#[test]
fn a_hidden_apps_path_and_icon_come_from_its_window_else_its_identity() {
    let mut engine = DockEngine::new(Settings::default());
    let id = stem_core::app_id_for(&code_window());
    assert_eq!(id, AppId::for_exe(Path::new(CODE_EXE)));
    assert_eq!(hidden_app_facts(&engine, &id), (None, None));

    // A window without an icon of its own and no identity yet: its
    // path, and the program's own icon.
    let program_icon = Some(IconKey(format!("exe:{CODE_EXE}")));
    engine.apply(EngineInput::Platform(PlatformEvent::WindowAdded(
        code_window(),
    )));
    assert_eq!(
        hidden_app_facts(&engine, &id),
        (Some(CODE_EXE.into()), program_icon.clone())
    );
    // A window icon (`hwnd:`) is never kept: its handle outlives the
    // window and may be another process's later.
    let with_icon = RawWindow {
        icon_key: Some(IconKey("hwnd:50:5050".into())),
        ..code_window()
    };
    engine.apply(EngineInput::Platform(PlatformEvent::WindowUpdated(
        with_icon.clone(),
    )));
    assert_eq!(hidden_app_facts(&engine, &id).1, program_icon);
    // Nor is it without a path to stand in: no icon then.
    let unreadable = RawWindow {
        id: WindowId(51),
        exe_path: None,
        app_user_model_id: None,
        class_name: "ProtectedClass".into(),
        icon_key: Some(IconKey("hwnd:51:5151".into())),
        ..with_icon
    };
    let protected = stem_core::app_id_for(&unreadable);
    engine.apply(EngineInput::Platform(PlatformEvent::WindowAdded(
        unreadable,
    )));
    engine.apply(EngineInput::IdentityResolved {
        app_id: protected.clone(),
        identity: AppIdentity {
            id: protected.clone(),
            display_name: "Protected".into(),
            icon_key: Some(IconKey("hwnd:51:5151".into())),
            launch: None,
        },
    });
    assert_eq!(hidden_app_facts(&engine, &protected), (None, None));
    // The identity's icon wins, as on the item; the path stays the window's.
    let shortcut_icon = Some(IconKey(format!("file:{CODE_LNK}")));
    engine.apply(EngineInput::IdentityResolved {
        app_id: id.clone(),
        identity: AppIdentity {
            id: id.clone(),
            display_name: "Visual Studio Code".into(),
            icon_key: shortcut_icon.clone(),
            launch: Some(LaunchTarget::Shortcut {
                path: PathBuf::from(CODE_LNK),
            }),
        },
    });
    assert_eq!(
        hidden_app_facts(&engine, &id),
        (Some(CODE_EXE.into()), shortcut_icon.clone())
    );
    // No window left: the file the identity launches.
    engine.apply(EngineInput::Platform(PlatformEvent::WindowRemoved(
        WindowId(50),
    )));
    assert_eq!(
        hidden_app_facts(&engine, &id),
        (Some(CODE_LNK.into()), shortcut_icon)
    );

    // An app known only by its identity, which launches a program: that
    // program's path, as written.
    let program = Path::new(r"C:\Tools\Foo.EXE");
    let foo = AppId::for_exe(program);
    engine.apply(EngineInput::IdentityResolved {
        app_id: foo.clone(),
        identity: AppIdentity {
            id: foo.clone(),
            display_name: "Foo".into(),
            icon_key: None,
            launch: Some(LaunchTarget::Exe {
                path: program.to_path_buf(),
                args: vec![],
                working_dir: None,
            }),
        },
    });
    assert_eq!(
        hidden_app_facts(&engine, &foo),
        (
            Some(r"C:\Tools\Foo.EXE".into()),
            Some(IconKey(r"exe:C:\Tools\Foo.EXE".into()))
        )
    );
}

#[test]
fn an_aumid_apps_path_comes_from_its_identity_never_from_its_window() {
    const CALCULATOR: &str = "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App";
    const FRAME_HOST: &str = r"C:\Windows\System32\ApplicationFrameHost.exe";
    let mut engine = DockEngine::new(Settings::default());
    // Calculator as Windows shows it: a frame of the frame host's
    // process that names the app's AppUserModelID.
    let frame = RawWindow {
        id: WindowId(60),
        title: "Calculator".into(),
        class_name: "ApplicationFrameWindow".into(),
        process_id: 6060,
        exe_path: Some(PathBuf::from(FRAME_HOST)),
        app_user_model_id: Some(CALCULATOR.into()),
        icon_key: Some(IconKey("hwnd:60:6060".into())),
        ..terminal_window()
    };
    let calc = AppId::for_app_user_model_id(CALCULATOR);
    assert_eq!(stem_core::app_id_for(&frame), calc);
    engine.apply(EngineInput::Platform(PlatformEvent::WindowAdded(frame)));
    // No identity yet: neither the frame host's path nor an icon made
    // from it (the window's own `hwnd:` key is never kept either).
    assert_eq!(hidden_app_facts(&engine, &calc), (None, None));
    // The identity's logo; a Store app's identity names no file.
    let logo = Some(IconKey(format!("aumid:{CALCULATOR}")));
    engine.apply(EngineInput::IdentityResolved {
        app_id: calc.clone(),
        identity: AppIdentity {
            id: calc.clone(),
            display_name: "Calculator".into(),
            icon_key: logo.clone(),
            launch: Some(LaunchTarget::StoreApp {
                app_user_model_id: CALCULATOR.into(),
            }),
        },
    });
    assert_eq!(hidden_app_facts(&engine, &calc), (None, logo.clone()));

    // The Terminal: an `aumid:` id too, so its window's exe under
    // WindowsApps is not asked either.
    let terminal = AppId::for_app_user_model_id(TERMINAL_AUMID);
    engine.apply(EngineInput::Platform(PlatformEvent::WindowAdded(
        terminal_window(),
    )));
    assert_eq!(hidden_app_facts(&engine, &terminal), (None, None));
    engine.apply(EngineInput::IdentityResolved {
        app_id: terminal.clone(),
        identity: terminal_identity(),
    });
    assert_eq!(
        hidden_app_facts(&engine, &terminal),
        (None, Some(IconKey(format!("aumid:{TERMINAL_AUMID}"))))
    );

    // A desktop program whose windows name their own AppUserModelID and
    // whose identity launches its exe: the identity's path, with its
    // file's icon standing in for the window's `hwnd:` key.
    const BROWSER: &str = r"C:\Program Files\Mozilla Firefox\firefox.exe";
    let browser = AppId::for_app_user_model_id("308046B0AF4A39CB");
    engine.apply(EngineInput::Platform(PlatformEvent::WindowAdded(
        RawWindow {
            id: WindowId(70),
            title: "Mozilla Firefox".into(),
            class_name: "MozillaWindowClass".into(),
            process_id: 7070,
            exe_path: Some(PathBuf::from(BROWSER)),
            app_user_model_id: Some("308046B0AF4A39CB".into()),
            icon_key: Some(IconKey("hwnd:70:7070".into())),
            ..terminal_window()
        },
    )));
    assert_eq!(hidden_app_facts(&engine, &browser), (None, None));
    engine.apply(EngineInput::IdentityResolved {
        app_id: browser.clone(),
        identity: AppIdentity {
            id: browser.clone(),
            display_name: "Firefox".into(),
            icon_key: None,
            launch: Some(exe_target(BROWSER)),
        },
    });
    assert_eq!(
        hidden_app_facts(&engine, &browser),
        (
            Some(BROWSER.into()),
            Some(IconKey(format!("exe:{BROWSER}")))
        )
    );
}
