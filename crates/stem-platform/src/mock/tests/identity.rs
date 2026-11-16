use std::path::PathBuf;

use stem_core::{
    AppId, AppIdentity, IconKey, LaunchTarget, PlatformEvent, RawWindow, Rect, WindowId,
};

use crate::Platform;
use crate::mock::MockPlatform;
use crate::mock::demo::demo_window;
use crate::mock::identity::apps_folder_knows;

#[test]
fn icons_are_valid_png_and_deterministic() {
    let p = MockPlatform::new();
    let a = p
        .icon_png(&IconKey("mock:exe:x".into()), 48)
        .unwrap()
        .unwrap();
    let b = p
        .icon_png(&IconKey("mock:exe:x".into()), 48)
        .unwrap()
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(&a[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(p.icon_png(&IconKey::shell("start"), 48).unwrap(), None);
}

#[test]
fn identify_target_uses_canonical_app_ids() {
    let p = MockPlatform::new();
    let id = p
        .identify_target(&LaunchTarget::Exe {
            path: PathBuf::from(r"C:\A\b.EXE"),
            args: vec![],
            working_dir: None,
        })
        .unwrap();
    assert_eq!(id.id, AppId::for_exe(std::path::Path::new(r"c:/a/b.exe")));
    assert_eq!(id.display_name, "b");
}

#[test]
fn a_packaged_app_in_a_frame_host_window_is_the_store_app_and_names_no_file() {
    // As `windows::identity::identify_window` resolves an AUMID
    // the AppsFolder knows.
    const CALCULATOR: &str = "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App";
    const FRAME_HOST: &str = r"C:\Windows\System32\ApplicationFrameHost.exe";
    let p = MockPlatform::new();
    let frame = |id: u64, exe: &str, aumid: Option<&str>| RawWindow {
        id: WindowId(id),
        title: "Calculator".into(),
        class_name: "ApplicationFrameWindow".into(),
        app_user_model_id: aumid.map(str::to_owned),
        ..demo_window(id, exe, "", false, Rect::new(300, 100, 400, 600))
    };
    // Calculator as Windows shows it: a frame of the frame host's
    // process that names the app's AppUserModelID.
    p.push(PlatformEvent::WindowAdded(frame(
        60,
        FRAME_HOST,
        Some(CALCULATOR),
    )));
    let calc = AppId::for_app_user_model_id(CALCULATOR);
    let store_app = AppIdentity {
        id: calc.clone(),
        display_name: "WindowsCalculator".into(),
        icon_key: Some(IconKey(format!("mock:{}", calc.as_str()))),
        launch: Some(LaunchTarget::StoreApp {
            app_user_model_id: CALCULATOR.into(),
        }),
    };
    assert_eq!(p.identify_window(WindowId(60)).unwrap(), store_app);
    // A pin of the Store app is the same identity, as both come from
    // the AppsFolder on Windows; blanks around the id are not kept.
    let pinned = LaunchTarget::StoreApp {
        app_user_model_id: format!(" {CALCULATOR} "),
    };
    assert_eq!(p.identify_target(&pinned).unwrap(), store_app);

    // A desktop program with an AppUserModelID of its own, which the
    // mock's shell does not know: still an `aumid:` id, but named after
    // and launching its exe, as on Windows for an AUMID outside the
    // AppsFolder.
    const FIREFOX: &str = r"C:\Program Files\Mozilla Firefox\firefox.exe";
    p.push(PlatformEvent::WindowAdded(frame(
        70,
        FIREFOX,
        Some("308046B0AF4A39CB"),
    )));
    let firefox = p.identify_window(WindowId(70)).unwrap();
    assert_eq!(firefox.id, AppId::for_app_user_model_id("308046B0AF4A39CB"));
    assert_eq!(firefox.display_name, "firefox");
    assert_eq!(
        firefox.launch,
        Some(LaunchTarget::Exe {
            path: PathBuf::from(FIREFOX),
            args: vec![],
            working_dir: None,
        })
    );
    // No AppUserModelID: unchanged, the exe's identity.
    p.push(PlatformEvent::WindowAdded(frame(80, FIREFOX, None)));
    let plain = p.identify_window(WindowId(80)).unwrap();
    assert_eq!(plain.id, AppId::for_exe(std::path::Path::new(FIREFOX)));
    assert_eq!(plain.display_name, "firefox");
    assert_eq!(plain.launch, firefox.launch);
}

#[test]
fn the_mock_shell_knows_packaged_app_user_model_ids_only() {
    for known in [
        "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
        " Microsoft.WindowsTerminal_8wekyb3d8bbwe!App ",
        "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel",
    ] {
        assert!(apps_folder_knows(known), "{known}");
    }
    for unknown in [
        "",
        "308046B0AF4A39CB",
        "Chrome.Profile1",
        "Microsoft.WindowsCalculator_8wekyb3d8bbwe",
        "Microsoft.WindowsCalculator!App",
        "_8wekyb3d8bbwe!App",
        "Microsoft.WindowsCalculator_!App",
        "Microsoft.WindowsCalculator_8wekyb3d8bbwe!",
        "Microsoft.WindowsCalculator_1.0_x64__8wekyb3d8bbwe!App",
    ] {
        assert!(!apps_folder_knows(unknown), "{unknown}");
    }
}
