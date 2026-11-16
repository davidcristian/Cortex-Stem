use pretty_assertions::assert_eq;

use super::*;

const OWN_PID: u32 = 4242;

/// A plain Win32 app window the way Notepad presents one.
fn notepad() -> WindowFacts {
    WindowFacts {
        process_id: 1000,
        own_process_id: OWN_PID,
        is_root: true,
        is_visible: true,
        class_name: "Notepad".into(),
        title: "Untitled - Notepad".into(),
        is_tool_window: false,
        is_app_window: false,
        is_no_activate: false,
        owner: Owner::None,
        cloak_flags: 0,
    }
}

/// Calculator: a UWP app hosted by an `ApplicationFrameWindow`.
fn calculator() -> WindowFacts {
    WindowFacts {
        class_name: UWP_FRAME_CLASS.into(),
        title: "Calculator".into(),
        ..notepad()
    }
}

#[test]
fn plain_app_window_is_eligible() {
    assert_eq!(evaluate(&notepad()), Ok(()));
    assert!(is_taskbar_candidate(&notepad()));
    assert_eq!(evaluate(&calculator()), Ok(()));
}

#[test]
fn our_own_windows_are_rejected_before_anything_else() {
    let facts = WindowFacts {
        process_id: OWN_PID,
        is_visible: false,
        is_root: false,
        ..notepad()
    };
    assert_eq!(evaluate(&facts), Err(Rejection::OwnProcess));
}

#[test]
fn child_windows_are_rejected() {
    let facts = WindowFacts {
        is_root: false,
        ..notepad()
    };
    assert_eq!(evaluate(&facts), Err(Rejection::NotRoot));
}

#[test]
fn hidden_windows_are_rejected() {
    let facts = WindowFacts {
        is_visible: false,
        ..notepad()
    };
    assert_eq!(evaluate(&facts), Err(Rejection::NotVisible));
}

#[test]
fn shell_classes_are_rejected_case_insensitively() {
    for class in ["Progman", "WorkerW", "Shell_TrayWnd", "shell_traywnd"] {
        let facts = WindowFacts {
            class_name: class.into(),
            title: "Program Manager".into(),
            ..notepad()
        };
        assert_eq!(evaluate(&facts), Err(Rejection::ShellClass), "{class}");
    }
    // The UWP core window is rejected; its ApplicationFrameWindow host is the button.
    let core = WindowFacts {
        class_name: "Windows.UI.Core.CoreWindow".into(),
        title: "Calculator".into(),
        ..notepad()
    };
    assert_eq!(evaluate(&core), Err(Rejection::ShellClass));
    assert!(is_shell_class("PROGMAN"));
    assert!(!is_shell_class("Notepad"));
}

#[test]
fn tool_windows_are_rejected_unless_app_window() {
    let tool = WindowFacts {
        is_tool_window: true,
        ..notepad()
    };
    assert_eq!(evaluate(&tool), Err(Rejection::ToolWindow));
    let forced = WindowFacts {
        is_app_window: true,
        ..tool
    };
    assert_eq!(evaluate(&forced), Ok(()));
}

#[test]
fn no_activate_windows_are_rejected_unless_app_window() {
    // An overlay, an OSD, another launcher's panel: no taskbar button.
    let overlay = WindowFacts {
        is_no_activate: true,
        title: "Overlay".into(),
        ..notepad()
    };
    assert_eq!(evaluate(&overlay), Err(Rejection::NoActivate));
    // "To force the window to appear on the taskbar, use WS_EX_APPWINDOW."
    let forced = WindowFacts {
        is_app_window: true,
        ..overlay.clone()
    };
    assert_eq!(evaluate(&forced), Ok(()));
    // A tool window that is also no-activate reports the tool window.
    let tool = WindowFacts {
        is_tool_window: true,
        ..overlay
    };
    assert_eq!(evaluate(&tool), Err(Rejection::ToolWindow));
}

#[test]
fn owned_windows_need_a_hidden_owner_and_app_window_style() {
    let dialog = WindowFacts {
        owner: Owner::Visible,
        title: "Save As".into(),
        ..notepad()
    };
    assert_eq!(evaluate(&dialog), Err(Rejection::Owned));

    // Hidden owner alone is not enough (that is the trick apps use to
    // *avoid* a taskbar button).
    let hidden_owner = WindowFacts {
        owner: Owner::Hidden,
        ..notepad()
    };
    assert_eq!(evaluate(&hidden_owner), Err(Rejection::Owned));

    // Hidden owner + WS_EX_APPWINDOW: the app asked for a button.
    let opted_in = WindowFacts {
        owner: Owner::Hidden,
        is_app_window: true,
        ..notepad()
    };
    assert_eq!(evaluate(&opted_in), Ok(()));

    // Visible owner + WS_EX_APPWINDOW (a WinForms form shown with an
    // owner and ShowInTaskbar): the style forces the button.
    let visible_owner_app = WindowFacts {
        owner: Owner::Visible,
        is_app_window: true,
        ..notepad()
    };
    assert_eq!(evaluate(&visible_owner_app), Ok(()));
    // It still needs a title and no cloak of its own.
    let untitled = WindowFacts {
        title: String::new(),
        ..visible_owner_app.clone()
    };
    assert_eq!(evaluate(&untitled), Err(Rejection::EmptyTitle));
    let cloaked = WindowFacts {
        cloak_flags: CLOAKED_APP,
        ..visible_owner_app
    };
    assert_eq!(evaluate(&cloaked), Err(Rejection::Cloaked));
}

#[test]
fn shell_cloaked_windows_stay_eligible_and_report_cloaked() {
    // On another virtual desktop: the engine decides whether to show it.
    let other_desktop = WindowFacts {
        cloak_flags: CLOAKED_SHELL,
        ..notepad()
    };
    assert_eq!(evaluate(&other_desktop), Ok(()));
    assert!(other_desktop.is_cloaked());
    assert!(other_desktop.is_shell_cloaked());

    let mixed = WindowFacts {
        cloak_flags: CLOAKED_SHELL | CLOAKED_INHERITED,
        ..notepad()
    };
    assert_eq!(evaluate(&mixed), Ok(()));
}

#[test]
fn app_cloaked_windows_are_rejected_unless_uwp_frame() {
    for flags in [
        CLOAKED_APP,
        CLOAKED_INHERITED,
        CLOAKED_APP | CLOAKED_INHERITED,
    ] {
        let facts = WindowFacts {
            cloak_flags: flags,
            ..notepad()
        };
        assert_eq!(evaluate(&facts), Err(Rejection::Cloaked), "{flags:#x}");
    }
    let suspended_uwp = WindowFacts {
        cloak_flags: CLOAKED_APP,
        ..calculator()
    };
    assert_eq!(evaluate(&suspended_uwp), Ok(()));
    assert!(suspended_uwp.is_cloaked());
}

#[test]
fn empty_titles_are_rejected_unless_uwp_frame() {
    for title in ["", "   ", "\t"] {
        let facts = WindowFacts {
            title: title.into(),
            ..notepad()
        };
        assert_eq!(evaluate(&facts), Err(Rejection::EmptyTitle), "{title:?}");
    }
    let frame = WindowFacts {
        title: String::new(),
        ..calculator()
    };
    assert_eq!(evaluate(&frame), Ok(()));
    // Class names are case-insensitive in Win32.
    let frame_lower = WindowFacts {
        class_name: "applicationframewindow".into(),
        ..frame
    };
    assert_eq!(evaluate(&frame_lower), Ok(()));
}

#[test]
fn rules_apply_in_table_order() {
    // Visible test: an invisible tool window reports NotVisible, not ToolWindow.
    let facts = WindowFacts {
        is_visible: false,
        is_tool_window: true,
        title: String::new(),
        ..notepad()
    };
    assert_eq!(evaluate(&facts), Err(Rejection::NotVisible));
    // An owned, cloaked, untitled window reports Owned first.
    let facts = WindowFacts {
        owner: Owner::Visible,
        cloak_flags: CLOAKED_APP,
        title: String::new(),
        ..notepad()
    };
    assert_eq!(evaluate(&facts), Err(Rejection::Owned));
}

#[test]
fn elevated_window_facts_do_not_need_a_process_path() {
    // Eligibility never looks at the exe path: an elevated app (whose path
    // we cannot read) is still a candidate.
    let elevated = WindowFacts {
        class_name: "TaskManagerWindow".into(),
        title: "Task Manager".into(),
        ..notepad()
    };
    assert_eq!(evaluate(&elevated), Ok(()));
}
