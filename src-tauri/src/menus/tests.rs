use stem_core::{AppId, ShellAction, WindowId};

use super::actions::{ID_PREFIX, MenuAction, supersedes_focus};
use super::build::{MAX_TITLE_CHARS, has_launch_target, menu_text, mute_text};

#[test]
fn menu_ids_round_trip_through_parse() {
    let actions = [
        MenuAction::Activate(WindowId(0x1_0042)),
        MenuAction::Launch(AppId("exe:c:/program files/app/app.exe".into())),
        MenuAction::Pin(AppId(
            "aumid:Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into(),
        )),
        MenuAction::Unpin(AppId("uri:ms-settings:".into())),
        MenuAction::Hide(AppId("exe:c:/windows/notepad.exe".into())),
        MenuAction::Hide(AppId("class:notepad".into())),
        MenuAction::CloseAll {
            dock: "dock-DISPLAY3".into(),
            app: AppId("class:notepad".into()),
        },
        MenuAction::CloseAll {
            dock: "dock".into(),
            app: AppId("exe:c:/windows/notepad.exe".into()),
        },
        MenuAction::Shell(ShellAction::EmptyRecycleBin),
        MenuAction::Shell(ShellAction::OpenQuickSettings),
        MenuAction::Shell(ShellAction::OpenNotificationCenter),
        MenuAction::Shell(ShellAction::OpenDateTimeSettings),
        MenuAction::Shell(ShellAction::OpenPowerSettings),
        MenuAction::Shell(ShellAction::OpenSoundSettings),
        MenuAction::Shell(ShellAction::OpenVolumeMixer),
        MenuAction::Mute(true),
        MenuAction::Mute(false),
        MenuAction::Settings,
        MenuAction::Quit,
    ];
    for action in actions {
        let id = action.id();
        assert!(id.starts_with(ID_PREFIX), "{id}");
        assert_eq!(MenuAction::parse(&id), Some(action), "{id}");
    }
    assert_eq!(
        MenuAction::Shell(ShellAction::OpenStartMenu).id(),
        "ctx:shell:openStartMenu"
    );
    assert_eq!(
        MenuAction::Shell(ShellAction::OpenQuickSettings).id(),
        "ctx:shell:openQuickSettings"
    );
    assert_eq!(
        MenuAction::Shell(ShellAction::OpenNotificationCenter).id(),
        "ctx:shell:openNotificationCenter"
    );
    assert_eq!(
        MenuAction::Shell(ShellAction::OpenDateTimeSettings).id(),
        "ctx:shell:openDateTimeSettings"
    );
    assert_eq!(
        MenuAction::Shell(ShellAction::OpenPowerSettings).id(),
        "ctx:shell:openPowerSettings"
    );
    assert_eq!(
        MenuAction::Shell(ShellAction::OpenSoundSettings).id(),
        "ctx:shell:openSoundSettings"
    );
    assert_eq!(
        MenuAction::Shell(ShellAction::OpenVolumeMixer).id(),
        "ctx:shell:openVolumeMixer"
    );
    assert_eq!(MenuAction::Mute(true).id(), "ctx:mute:on");
    assert_eq!(MenuAction::Mute(false).id(), "ctx:mute:off");
    assert_eq!(mute_text(true), "Mute");
    assert_eq!(mute_text(false), "Unmute");
    assert_eq!(MenuAction::Activate(WindowId(7)).id(), "ctx:activate:7");
    assert_eq!(
        MenuAction::Hide(AppId("exe:c:/windows/notepad.exe".into())).id(),
        "ctx:hide:exe:c:/windows/notepad.exe"
    );
    assert_eq!(
        MenuAction::CloseAll {
            dock: "dock-DISPLAY3".into(),
            app: AppId("exe:c:/windows/notepad.exe".into()),
        }
        .id(),
        "ctx:close-all:dock-DISPLAY3:exe:c:/windows/notepad.exe"
    );
}

#[test]
fn foreign_and_malformed_ids_do_not_parse() {
    for id in [
        "settings",
        "quit",
        "ctx:",
        "ctx:nope",
        "ctx:activate:",
        "ctx:activate:x",
        "ctx:launch:",
        "ctx:hide:",
        "ctx:hide",
        "ctx:shell:OpenStartMenu",
        "ctx:shell:",
        "ctx:close-all",
        "ctx:close-all:dock",
        "ctx:close-all:dock:",
        "ctx:close-all::exe:c:/a.exe",
        "ctx:mute",
        "ctx:mute:",
        "ctx:mute:true",
        "ctx:mute:On",
    ] {
        assert_eq!(MenuAction::parse(id), None, "{id}");
    }
}

#[test]
fn only_entries_that_act_on_windows_supersede_the_focus_shortcut() {
    let notepad = || AppId("exe:c:/windows/notepad.exe".into());
    // They activate, launch or act on windows, as a click does.
    for action in [
        MenuAction::Activate(WindowId(0x1_0042)),
        MenuAction::Launch(notepad()),
        MenuAction::CloseAll {
            dock: "dock".into(),
            app: notepad(),
        },
        MenuAction::Shell(ShellAction::OpenStartMenu),
        MenuAction::Shell(ShellAction::EmptyRecycleBin),
        MenuAction::Shell(ShellAction::OpenSoundSettings),
        MenuAction::Shell(ShellAction::OpenVolumeMixer),
    ] {
        assert!(supersedes_focus(&action), "{action:?}");
    }
    // Settings changes (Hide among them) and Stem's own entries:
    // Escape still gives the focus back to the window in front at the press.
    for action in [
        MenuAction::Pin(notepad()),
        MenuAction::Unpin(notepad()),
        MenuAction::Hide(notepad()),
        // The volume's mute touches no window.
        MenuAction::Mute(true),
        MenuAction::Mute(false),
        MenuAction::Settings,
        MenuAction::Quit,
    ] {
        assert!(!supersedes_focus(&action), "{action:?}");
    }
}

#[test]
fn menu_text_truncates_escapes_and_never_goes_empty() {
    assert_eq!(menu_text("  Downloads  "), "Downloads");
    assert_eq!(menu_text(""), "(untitled)");
    assert_eq!(menu_text("Fish & Chips"), "Fish && Chips");
    let long = "x".repeat(MAX_TITLE_CHARS + 5);
    let text = menu_text(&long);
    assert_eq!(text.chars().count(), MAX_TITLE_CHARS + 1);
    assert!(text.ends_with('…'));
    // Exactly the limit: untouched (multi-byte characters count as one).
    let exact = "é".repeat(MAX_TITLE_CHARS);
    assert_eq!(menu_text(&exact), exact);
}

#[test]
fn launchability_follows_the_id_namespace() {
    assert!(has_launch_target(&AppId(
        "exe:c:/windows/notepad.exe".into()
    )));
    assert!(has_launch_target(&AppId("aumid:Some.App!App".into())));
    assert!(!has_launch_target(&AppId("class:someclass".into())));
    assert!(!has_launch_target(&AppId("uri:ms-settings:".into())));
}
