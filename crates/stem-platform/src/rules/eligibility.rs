//! Which windows belong in the dock.
//!
//! `windows::enumerate` collects plain facts about a window (styles, owner,
//! class, title, cloak state, process) into a [`WindowFacts`] and asks
//! [`is_taskbar_candidate`] (or [`evaluate`], which also says *why* a window
//! was rejected). The table mirrors what the stock Windows 11 taskbar shows.
//! Keep the rule table and its tests here so the decision can be read and
//! tested on any host; the OS calls that produce the facts live in
//! `windows/enumerate.rs`.
//!
//! Rule table (first match wins, top to bottom):
//!
//! | # | Fact                                                          | Verdict |
//! |---|---------------------------------------------------------------|---------|
//! | 1 | the window belongs to our own process                         | reject `OwnProcess` |
//! | 2 | `GetAncestor(GA_ROOT) != hwnd` (a child window)               | reject `NotRoot` |
//! | 3 | not `IsWindowVisible`                                         | reject `NotVisible` |
//! | 4 | class is a shell class ([`SHELL_CLASSES`])                    | reject `ShellClass` |
//! | 5 | `WS_EX_TOOLWINDOW` without `WS_EX_APPWINDOW`                  | reject `ToolWindow` |
//! | 6 | has an owner, unless the owner is invisible and `WS_EX_APPWINDOW` is set | reject `Owned` |
//! | 7 | cloaked for a reason other than the shell (see below), unless the class is [`UWP_FRAME_CLASS`] | reject `Cloaked` |
//! | 8 | empty title, unless the class is [`UWP_FRAME_CLASS`]          | reject `EmptyTitle` |
//! | 9 | otherwise                                                     | eligible |
//!
//! Cloaking: `DWMWA_CLOAKED` reports *why* a window is cloaked. The shell
//! cloaks windows that live on another virtual desktop ([`CLOAKED_SHELL`]);
//! those stay eligible and are reported with `RawWindow::is_cloaked = true`
//! so the engine can hide them per `behavior.show_only_current_desktop`.
//! Windows cloaked by the application itself ([`CLOAKED_APP`]) or by
//! inheritance ([`CLOAKED_INHERITED`]) are not shown by the taskbar either,
//! with one exception: UWP frame hosts (`ApplicationFrameWindow`) stay
//! cloaked while the app is suspended and the taskbar still lists them.
//! *Where* a cloaked window is, is not an eligibility question: the shell
//! also cloaks during a desktop-switch animation and the frame a closed
//! Store app can leave behind, so `windows/virtual_desktop.rs` asks the
//! shell for every eligible window and reports the answer as
//! `RawWindow::on_current_desktop`, which the engine prefers to the
//! cloak state when it is known.
//!
//! The constants mirror `dwmapi.h`; `windows/enumerate.rs` asserts at
//! compile time that they equal the SDK values.

/// `DWM_CLOAKED_APP`: the window cloaked itself.
pub const CLOAKED_APP: u32 = 0x1;
/// `DWM_CLOAKED_SHELL`: the shell cloaked the window (other virtual desktop).
pub const CLOAKED_SHELL: u32 = 0x2;
/// `DWM_CLOAKED_INHERITED`: cloaked because its owner is cloaked.
pub const CLOAKED_INHERITED: u32 = 0x4;

/// Window class of the frame that hosts packaged (UWP / Store) apps. These
/// windows may legitimately have an empty title or be cloaked while the app
/// is suspended, and the taskbar still shows them.
pub const UWP_FRAME_CLASS: &str = "ApplicationFrameWindow";

/// Top-level shell windows that are visible but never taskbar buttons:
/// the desktop (`Progman`, `WorkerW`), the taskbars, and the inner core
/// window of UWP apps (its `ApplicationFrameWindow` host is the button).
/// Compared case-insensitively, like window class names are.
pub const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
];

/// The owner relationship of a window (`GetWindow(GW_OWNER)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Owner {
    /// No owner window.
    #[default]
    None,
    /// Owned, and the owner is visible.
    Visible,
    /// Owned, and the owner is hidden (the "hidden owner" taskbar trick).
    Hidden,
}

/// Plain facts about one top-level window, gathered by `windows::enumerate`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowFacts {
    /// Process that owns the window (`GetWindowThreadProcessId`).
    pub process_id: u32,
    /// Our own process id (`GetCurrentProcessId`).
    pub own_process_id: u32,
    /// `GetAncestor(GA_ROOT) == hwnd`.
    pub is_root: bool,
    /// `IsWindowVisible`.
    pub is_visible: bool,
    /// `GetClassNameW`.
    pub class_name: String,
    /// `GetWindowTextW`.
    pub title: String,
    /// `WS_EX_TOOLWINDOW` is set.
    pub is_tool_window: bool,
    /// `WS_EX_APPWINDOW` is set.
    pub is_app_window: bool,
    pub owner: Owner,
    /// `DWMWA_CLOAKED` flags (`0` when not cloaked or when DWM refused).
    pub cloak_flags: u32,
}

impl WindowFacts {
    /// Cloaked for any reason; reported as `RawWindow::is_cloaked`.
    pub fn is_cloaked(&self) -> bool {
        self.cloak_flags != 0
    }

    /// Cloaked by the shell, i.e. parked on another virtual desktop.
    pub fn is_shell_cloaked(&self) -> bool {
        self.cloak_flags & CLOAKED_SHELL != 0
    }

    pub fn is_uwp_frame(&self) -> bool {
        self.class_name.eq_ignore_ascii_case(UWP_FRAME_CLASS)
    }
}

/// Why a window is not a taskbar candidate (for debug logging).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    OwnProcess,
    NotRoot,
    NotVisible,
    ShellClass,
    ToolWindow,
    Owned,
    Cloaked,
    EmptyTitle,
}

/// Whether `class_name` is one of [`SHELL_CLASSES`] (case-insensitive).
pub fn is_shell_class(class_name: &str) -> bool {
    SHELL_CLASSES
        .iter()
        .any(|c| c.eq_ignore_ascii_case(class_name))
}

/// The rule table from the module documentation.
pub fn evaluate(facts: &WindowFacts) -> Result<(), Rejection> {
    if facts.process_id == facts.own_process_id {
        return Err(Rejection::OwnProcess);
    }
    if !facts.is_root {
        return Err(Rejection::NotRoot);
    }
    if !facts.is_visible {
        return Err(Rejection::NotVisible);
    }
    if is_shell_class(&facts.class_name) {
        return Err(Rejection::ShellClass);
    }
    if facts.is_tool_window && !facts.is_app_window {
        return Err(Rejection::ToolWindow);
    }
    match facts.owner {
        Owner::None => {}
        Owner::Hidden if facts.is_app_window => {}
        Owner::Hidden | Owner::Visible => return Err(Rejection::Owned),
    }
    if facts.is_cloaked() && !facts.is_shell_cloaked() && !facts.is_uwp_frame() {
        return Err(Rejection::Cloaked);
    }
    if facts.title.trim().is_empty() && !facts.is_uwp_frame() {
        return Err(Rejection::EmptyTitle);
    }
    Ok(())
}

/// `true` when the stock taskbar would show a button for this window.
pub fn is_taskbar_candidate(facts: &WindowFacts) -> bool {
    evaluate(facts).is_ok()
}

#[cfg(test)]
mod tests {
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

        // Visible owner + WS_EX_APPWINDOW: still owned, still rejected.
        let visible_owner_app = WindowFacts {
            owner: Owner::Visible,
            is_app_window: true,
            ..notepad()
        };
        assert_eq!(evaluate(&visible_owner_app), Err(Rejection::Owned));
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
}
