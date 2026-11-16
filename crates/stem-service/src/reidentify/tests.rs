use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;

use super::*;

const TERMINAL_EXE: &str = r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.24.12741.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe";
const TERMINAL_AUMID: &str = "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App";

fn exe_pin(path: &str, args: &[&str]) -> PinnedApp {
    PinnedApp {
        id: AppId::for_exe(Path::new(path)),
        label: None,
        launch: LaunchTarget::Exe {
            path: PathBuf::from(path),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            working_dir: None,
        },
    }
}

fn store_pin(aumid: &str) -> PinnedApp {
    PinnedApp {
        id: AppId::for_app_user_model_id(aumid),
        label: None,
        launch: LaunchTarget::StoreApp {
            app_user_model_id: aumid.to_owned(),
        },
    }
}

fn identity(id: AppId) -> AppIdentity {
    AppIdentity {
        id,
        display_name: "Terminal".into(),
        icon_key: None,
        launch: None,
    }
}

fn found(pin: &PinnedApp, aumid: &str) -> Reidentified {
    Reidentified {
        old: pin.id.clone(),
        aumid: aumid.to_owned(),
    }
}

#[test]
fn only_argument_free_exe_pins_under_windows_apps_are_candidates() {
    let stale = exe_pin(TERMINAL_EXE, &[]);
    assert_eq!(
        stale.id.as_str(),
        "exe:c:/program files/windowsapps/microsoft.windowsterminal_1.24.12741.0_x64__8wekyb3d8bbwe/windowsterminal.exe"
    );
    assert!(is_candidate(&stale));
    // A working directory does not matter to a packaged app.
    let mut with_dir = stale.clone();
    if let LaunchTarget::Exe { working_dir, .. } = &mut with_dir.launch {
        *working_dir = Some(PathBuf::from(r"C:\Users\me"));
    }
    assert!(is_candidate(&with_dir));

    // Arguments would be lost; other places, kinds and ids are not stale.
    assert!(!is_candidate(&exe_pin(TERMINAL_EXE, &["-p", "PowerShell"])));
    assert!(!is_candidate(&exe_pin(
        r"C:\Program Files\Mozilla Firefox\firefox.exe",
        &[]
    )));
    assert!(!is_candidate(&exe_pin(
        r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\wt.exe",
        &[]
    )));
    // An inbox exe Windows may redirect to a packaged app is asked about
    // too, unless it carries arguments.
    assert!(is_candidate(&exe_pin(r"C:\Windows\notepad.exe", &[])));
    assert!(is_candidate(&exe_pin(
        r"C:\Windows\System32\notepad.exe",
        &[]
    )));
    assert!(!is_candidate(&exe_pin(
        r"C:\Windows\System32\notepad.exe",
        &[r"C:\notes.txt"]
    )));
    assert!(!is_candidate(&store_pin(TERMINAL_AUMID)));
    let mut aumid_id = stale.clone();
    aumid_id.id = AppId::for_app_user_model_id(TERMINAL_AUMID);
    assert!(!is_candidate(&aumid_id));
    let shortcut = PinnedApp {
        launch: LaunchTarget::Shortcut {
            path: PathBuf::from(TERMINAL_EXE),
        },
        ..stale.clone()
    };
    assert!(!is_candidate(&shortcut));
    let uri = PinnedApp {
        launch: LaunchTarget::Uri {
            uri: "ms-settings:".into(),
        },
        ..stale
    };
    assert!(!is_candidate(&uri));
}

#[test]
fn only_a_different_aumid_answer_counts() {
    let stale = exe_pin(TERMINAL_EXE, &[]);
    assert_eq!(
        reidentified(
            &stale,
            &identity(AppId::for_app_user_model_id(TERMINAL_AUMID))
        ),
        Some(found(&stale, TERMINAL_AUMID))
    );
    // The platform kept the exe identity: no match.
    assert_eq!(reidentified(&stale, &identity(stale.id.clone())), None);
    // Anything that is not an AUMID, or a blank one, is no answer either.
    assert_eq!(
        reidentified(&stale, &identity(AppId::for_exe(Path::new(r"C:\x.exe")))),
        None
    );
    assert_eq!(
        reidentified(&stale, &identity(AppId("uri:ms-settings:".into()))),
        None
    );
    assert_eq!(
        reidentified(&stale, &identity(AppId("aumid:".into()))),
        None
    );
    // An AUMID pin that is asked again keeps its id.
    let pinned = store_pin(TERMINAL_AUMID);
    assert_eq!(reidentified(&pinned, &identity(pinned.id.clone())), None);
}

#[test]
fn a_stale_pin_is_rewritten_in_place_and_keeps_its_label() {
    let firefox = exe_pin(r"C:\Program Files\Mozilla Firefox\firefox.exe", &[]);
    let mut stale = exe_pin(TERMINAL_EXE, &[]);
    stale.label = Some("Shell".into());
    let calculator = store_pin("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App");
    let mut pinned = vec![firefox.clone(), stale.clone(), calculator.clone()];

    let rewrites = rewrite_pins(&mut pinned, &[found(&stale, TERMINAL_AUMID)]);

    let new = AppId::for_app_user_model_id(TERMINAL_AUMID);
    assert_eq!(
        rewrites,
        vec![PinRewrite::Replaced {
            old: stale.id.clone(),
            new: new.clone()
        }]
    );
    assert_eq!(
        pinned,
        vec![
            firefox,
            PinnedApp {
                id: new,
                label: Some("Shell".into()),
                launch: LaunchTarget::StoreApp {
                    app_user_model_id: TERMINAL_AUMID.into()
                },
            },
            calculator,
        ]
    );
    // Nothing left to do: applying the same answer again changes nothing.
    let before = pinned.clone();
    assert!(rewrite_pins(&mut pinned, &[found(&stale, TERMINAL_AUMID)]).is_empty());
    assert_eq!(pinned, before);
}

#[test]
fn a_stale_pin_of_an_app_that_is_already_pinned_is_dropped() {
    let current = store_pin(TERMINAL_AUMID);
    let stale = exe_pin(TERMINAL_EXE, &[]);
    let mut pinned = vec![stale.clone(), current.clone()];
    let rewrites = rewrite_pins(&mut pinned, &[found(&stale, TERMINAL_AUMID)]);
    assert_eq!(
        rewrites,
        vec![PinRewrite::Dropped {
            old: stale.id.clone(),
            new: current.id.clone()
        }]
    );
    assert_eq!(pinned, vec![current]);
}

#[test]
fn two_stale_pins_of_one_app_become_one() {
    // Two versions of the Terminal pinned by an older build.
    let older = exe_pin(
        r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.23.12811.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe",
        &[],
    );
    let newer = exe_pin(TERMINAL_EXE, &[]);
    let notepad = store_pin("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App");
    let mut pinned = vec![older.clone(), notepad.clone(), newer.clone()];
    let rewrites = rewrite_pins(
        &mut pinned,
        &[found(&older, TERMINAL_AUMID), found(&newer, TERMINAL_AUMID)],
    );
    let new = AppId::for_app_user_model_id(TERMINAL_AUMID);
    assert_eq!(
        rewrites,
        vec![
            PinRewrite::Replaced {
                old: older.id.clone(),
                new: new.clone()
            },
            PinRewrite::Dropped {
                old: newer.id.clone(),
                new: new.clone()
            },
        ]
    );
    assert_eq!(
        pinned.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
        vec![new, notepad.id]
    );
}

#[test]
fn a_pin_removed_meanwhile_is_skipped() {
    let stale = exe_pin(TERMINAL_EXE, &[]);
    let other = store_pin("Microsoft.Paint_8wekyb3d8bbwe!App");
    let mut pinned = vec![other.clone()];
    assert!(rewrite_pins(&mut pinned, &[found(&stale, TERMINAL_AUMID)]).is_empty());
    assert_eq!(pinned, vec![other]);
}

#[test]
fn rewrites_read_as_log_lines() {
    let old = AppId("exe:c:/program files/windowsapps/x_1.0.0.0_x64__p/x.exe".into());
    let new = AppId::for_app_user_model_id("X_p!App");
    assert_eq!(
        PinRewrite::Replaced {
            old: old.clone(),
            new: new.clone()
        }
        .to_string(),
        "re-identified pin exe:c:/program files/windowsapps/x_1.0.0.0_x64__p/x.exe as aumid:X_p!App"
    );
    assert_eq!(
        PinRewrite::Dropped { old, new }.to_string(),
        "dropped pin exe:c:/program files/windowsapps/x_1.0.0.0_x64__p/x.exe: it is aumid:X_p!App, which is already pinned"
    );
}
