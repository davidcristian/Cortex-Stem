//! Whether the foreground window owns the whole screen.
//!
//! `windows::fullscreen` gathers the facts about the foreground window into
//! a [`FullscreenFacts`] and asks [`is_fullscreen`]; the events thread emits
//! `PlatformEvent::FullscreenChanged` when the answer changes. The rule is
//! plain data so it is unit-tested on every host.
//!
//! Rule (first match wins):
//!
//! | # | Fact                                                                 | Verdict |
//! |---|----------------------------------------------------------------------|---------|
//! | 1 | the shell reports `QUNS_BUSY`, `QUNS_RUNNING_D3D_FULL_SCREEN` or `QUNS_PRESENTATION_MODE` ([`NotificationState`]) | fullscreen |
//! | 2 | the window is minimised                                              | not |
//! | 3 | the window belongs to our own process (the dock, the settings window) | not |
//! | 4 | the window is a shell surface (`Progman`, `WorkerW`, the taskbars; [`eligibility::SHELL_CLASSES`]) | not |
//! | 5 | the window is maximised                                              | not |
//! | 6 | its frame covers its monitor's full bounds ([`covers_monitor`])      | fullscreen |
//! | 7 | otherwise                                                            | not |
//!
//! Rule 1 is the stock taskbar's own judgement (`SHQueryUserNotificationState`
//! answers from the shell's rude-window detection), so a game in exclusive
//! mode or a presentation counts even when the frame cannot be read. Rules
//! 2–6 are the backstop for windows the shell has not judged yet: a frame
//! that covers the monitor's *bounds* (not the work area) is a browser after
//! F11, a video player, a slideshow. A maximised window never qualifies by
//! geometry: with the stock taskbar auto-hidden the work area is the
//! whole monitor and every maximised window would cover it, while the user
//! still expects the dock at the edge, as the stock taskbar still pops up
//! over a maximised window but not over a fullscreen one.
//!
//! The `QUNS_*` codes mirror `shellapi.h`; `windows/fullscreen.rs` asserts
//! at compile time that they equal the SDK values.

use stem_core::Rect;

use super::eligibility;

/// `QUNS_NOT_PRESENT`: the screen saver runs, the station is locked, or
/// a non-active fast-user-switching session is in progress.
pub const QUNS_NOT_PRESENT: i32 = 1;
/// `QUNS_BUSY`: a fullscreen application is running or presentation
/// settings are applied.
pub const QUNS_BUSY: i32 = 2;
/// `QUNS_RUNNING_D3D_FULL_SCREEN`: a fullscreen (exclusive mode) Direct3D
/// application is running.
pub const QUNS_RUNNING_D3D_FULL_SCREEN: i32 = 3;
/// `QUNS_PRESENTATION_MODE`: the user activated Windows presentation
/// settings to block notifications and pop-up messages.
pub const QUNS_PRESENTATION_MODE: i32 = 4;
/// `QUNS_ACCEPTS_NOTIFICATIONS`: none of the other states.
pub const QUNS_ACCEPTS_NOTIFICATIONS: i32 = 5;
/// `QUNS_QUIET_TIME`: the first hour after a new user signs in.
pub const QUNS_QUIET_TIME: i32 = 6;
/// `QUNS_APP`: a Windows Store app is running (Windows 8 era).
pub const QUNS_APP: i32 = 7;

/// How many pixels a frame may fall short of the monitor's bounds on each
/// side and still count as covering it: frames are reported a pixel off
/// after a DPI change or by apps that size themselves one short.
pub const COVER_TOLERANCE_PX: i32 = 1;

/// What `SHQueryUserNotificationState` said, without the `windows` types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationState {
    /// The call failed (no shell, no session); the geometry decides alone.
    #[default]
    Unknown,
    NotPresent,
    Busy,
    RunningD3dFullScreen,
    PresentationMode,
    AcceptsNotifications,
    QuietTime,
    App,
}

impl NotificationState {
    /// From the raw `QUERY_USER_NOTIFICATION_STATE` value; an unknown code
    /// (a future Windows) is [`NotificationState::Unknown`].
    pub fn from_code(code: i32) -> Self {
        match code {
            QUNS_NOT_PRESENT => Self::NotPresent,
            QUNS_BUSY => Self::Busy,
            QUNS_RUNNING_D3D_FULL_SCREEN => Self::RunningD3dFullScreen,
            QUNS_PRESENTATION_MODE => Self::PresentationMode,
            QUNS_ACCEPTS_NOTIFICATIONS => Self::AcceptsNotifications,
            QUNS_QUIET_TIME => Self::QuietTime,
            QUNS_APP => Self::App,
            _ => Self::Unknown,
        }
    }

    /// The shell says a fullscreen application or a presentation owns the
    /// screen (rule 1).
    pub fn says_fullscreen(self) -> bool {
        matches!(
            self,
            Self::Busy | Self::RunningD3dFullScreen | Self::PresentationMode
        )
    }
}

/// Plain facts about the foreground window, gathered by `windows::fullscreen`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FullscreenFacts<'a> {
    /// The window's frame covers its monitor's full bounds ([`covers_monitor`]);
    /// `false` when either rectangle could not be read.
    pub covers: bool,
    pub notification_state: NotificationState,
    /// `GetClassNameW`.
    pub class_name: &'a str,
    /// The window belongs to our own process.
    pub own_process: bool,
    /// `IsIconic`.
    pub minimized: bool,
    /// `IsZoomed`: a maximised window fills the work area, never the screen
    /// in the user's sense.
    pub maximized: bool,
}

/// Whether `window` (a frame in screen coordinates) contains the whole of
/// `monitor_bounds`, each edge allowed to fall [`COVER_TOLERANCE_PX`] short.
/// Both rectangles are in the same physical-pixel space.
pub fn covers_monitor(window: Rect, monitor_bounds: Rect) -> bool {
    if window.width <= 0 || window.height <= 0 {
        return false;
    }
    let t = COVER_TOLERANCE_PX;
    window.x <= monitor_bounds.x + t
        && window.y <= monitor_bounds.y + t
        && window.right() >= monitor_bounds.right() - t
        && window.bottom() >= monitor_bounds.bottom() - t
}

/// The rule table from the module documentation.
pub fn is_fullscreen(facts: &FullscreenFacts<'_>) -> bool {
    if facts.notification_state.says_fullscreen() {
        return true;
    }
    if facts.minimized
        || facts.own_process
        || eligibility::is_shell_class(facts.class_name)
        || facts.maximized
    {
        return false;
    }
    facts.covers
}

#[cfg(test)]
mod tests;
