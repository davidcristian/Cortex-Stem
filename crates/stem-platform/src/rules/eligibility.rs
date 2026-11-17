//! Which windows belong in the dock.
//!
//! `windows::enumerate` collects plain facts about a window (styles, owner,
//! class, title, cloak state, process) into a [`WindowFacts`] and asks
//! [`is_taskbar_candidate`] (or [`evaluate`], which also says *why* a window
//! was rejected). The table mirrors what the stock Windows 11 taskbar shows.
//! Keep the rule table and its tests here so the decision can be read and
//! tested on any host; the OS calls that produce the facts live in
//! `windows/enumerate.rs` and `windows/enumerate/`.
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
//! | 6 | `WS_EX_NOACTIVATE` without `WS_EX_APPWINDOW`                  | reject `NoActivate` |
//! | 7 | has an owner (visible or hidden) without `WS_EX_APPWINDOW`    | reject `Owned` |
//! | 8 | cloaked for a reason other than the shell (see below), unless the class is [`UWP_FRAME_CLASS`] | reject `Cloaked` |
//! | 9 | empty title, unless the class is [`UWP_FRAME_CLASS`]          | reject `EmptyTitle` |
//! | 10 | otherwise                                                    | eligible |
//!
//! `WS_EX_APPWINDOW` "forces a top-level window onto the taskbar when the
//! window is visible" (Microsoft's Extended Window Styles), whoever owns it
//! and whatever its other styles: a WinForms or WPF form with an owner and
//! *ShowInTaskbar* has a taskbar button, so rules 5 to 7 do not apply to it
//! (rules 8 and 9 still do). Without it, a tool window, a no-activate window
//! ("does not appear on the taskbar by default") and an owned window have
//! no button.
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
//! The constants mirror `dwmapi.h`; `windows/enumerate/facts.rs` asserts at
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
    /// `WS_EX_NOACTIVATE` is set.
    pub is_no_activate: bool,
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
    NoActivate,
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
    // WS_EX_APPWINDOW asks for a button whatever the other styles and the
    // owner say (module docs).
    if !facts.is_app_window {
        if facts.is_tool_window {
            return Err(Rejection::ToolWindow);
        }
        if facts.is_no_activate {
            return Err(Rejection::NoActivate);
        }
        if facts.owner != Owner::None {
            return Err(Rejection::Owned);
        }
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
mod tests;
