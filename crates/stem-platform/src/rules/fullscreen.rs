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
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const MONITOR: Rect = Rect::new(0, 0, 2560, 1440);
    const SECOND: Rect = Rect::new(2560, -200, 1920, 1080);

    /// A browser after F11, as the shell has not judged it yet.
    fn browser() -> FullscreenFacts<'static> {
        FullscreenFacts {
            covers: true,
            notification_state: NotificationState::AcceptsNotifications,
            class_name: "MozillaWindowClass",
            own_process: false,
            minimized: false,
            maximized: false,
        }
    }

    #[test]
    fn notification_codes_mirror_shellapi() {
        assert_eq!(
            NotificationState::from_code(1),
            NotificationState::NotPresent
        );
        assert_eq!(NotificationState::from_code(2), NotificationState::Busy);
        assert_eq!(
            NotificationState::from_code(3),
            NotificationState::RunningD3dFullScreen
        );
        assert_eq!(
            NotificationState::from_code(4),
            NotificationState::PresentationMode
        );
        assert_eq!(
            NotificationState::from_code(5),
            NotificationState::AcceptsNotifications
        );
        assert_eq!(
            NotificationState::from_code(6),
            NotificationState::QuietTime
        );
        assert_eq!(NotificationState::from_code(7), NotificationState::App);
        for code in [0, -1, 8, i32::MAX] {
            assert_eq!(
                NotificationState::from_code(code),
                NotificationState::Unknown
            );
        }
        assert_eq!(NotificationState::default(), NotificationState::Unknown);
    }

    #[test]
    fn the_shell_states_that_mean_fullscreen() {
        for state in [
            NotificationState::Busy,
            NotificationState::RunningD3dFullScreen,
            NotificationState::PresentationMode,
        ] {
            assert!(state.says_fullscreen(), "{state:?}");
        }
        for state in [
            NotificationState::Unknown,
            NotificationState::NotPresent,
            NotificationState::AcceptsNotifications,
            NotificationState::QuietTime,
            NotificationState::App,
        ] {
            assert!(!state.says_fullscreen(), "{state:?}");
        }
    }

    #[test]
    fn a_frame_equal_to_the_monitor_covers_it() {
        assert!(covers_monitor(MONITOR, MONITOR));
        assert!(covers_monitor(SECOND, SECOND));
        // Larger than the monitor (a game's frame with invisible borders) too.
        assert!(covers_monitor(Rect::new(-8, -8, 2576, 1456), MONITOR));
    }

    #[test]
    fn one_pixel_short_on_any_side_still_covers_two_do_not() {
        assert!(covers_monitor(Rect::new(1, 0, 2559, 1440), MONITOR));
        assert!(covers_monitor(Rect::new(0, 1, 2560, 1439), MONITOR));
        assert!(covers_monitor(Rect::new(0, 0, 2559, 1440), MONITOR));
        assert!(covers_monitor(Rect::new(0, 0, 2560, 1439), MONITOR));
        assert!(covers_monitor(Rect::new(1, 1, 2558, 1438), MONITOR));
        assert!(!covers_monitor(Rect::new(2, 0, 2558, 1440), MONITOR));
        assert!(!covers_monitor(Rect::new(0, 2, 2560, 1438), MONITOR));
        assert!(!covers_monitor(Rect::new(0, 0, 2558, 1440), MONITOR));
        assert!(!covers_monitor(Rect::new(0, 0, 2560, 1438), MONITOR));
        assert_eq!(COVER_TOLERANCE_PX, 1);
    }

    #[test]
    fn a_maximised_frame_with_a_visible_taskbar_does_not_cover() {
        // 48 px taskbar: the frame stops at the work area's bottom edge (the
        // 8 px invisible borders hang over the other three edges).
        assert!(!covers_monitor(Rect::new(-8, -8, 2576, 1400), MONITOR));
        // A window on the other monitor covers nothing of this one.
        assert!(!covers_monitor(SECOND, MONITOR));
        // Empty or inverted frames never cover.
        assert!(!covers_monitor(Rect::new(0, 0, 0, 0), MONITOR));
        assert!(!covers_monitor(Rect::new(0, 0, -2560, -1440), MONITOR));
        assert!(!covers_monitor(Rect::default(), Rect::default()));
    }

    #[test]
    fn a_covering_window_is_fullscreen() {
        assert!(is_fullscreen(&browser()));
        // Whatever the shell says short of a fullscreen state.
        for state in [
            NotificationState::Unknown,
            NotificationState::NotPresent,
            NotificationState::QuietTime,
            NotificationState::App,
        ] {
            assert!(
                is_fullscreen(&FullscreenFacts {
                    notification_state: state,
                    ..browser()
                }),
                "{state:?}"
            );
        }
    }

    #[test]
    fn the_shell_verdict_wins_over_the_geometry() {
        for state in [
            NotificationState::Busy,
            NotificationState::RunningD3dFullScreen,
            NotificationState::PresentationMode,
        ] {
            // The frame could not be read (an exclusive-mode game), or the
            // window is one the geometry rule would exclude: still fullscreen.
            assert!(is_fullscreen(&FullscreenFacts {
                covers: false,
                notification_state: state,
                ..browser()
            }));
            assert!(is_fullscreen(&FullscreenFacts {
                notification_state: state,
                maximized: true,
                minimized: true,
                ..browser()
            }));
        }
    }

    #[test]
    fn minimised_own_shell_and_maximised_windows_are_not_fullscreen_by_geometry() {
        assert!(!is_fullscreen(&FullscreenFacts {
            minimized: true,
            ..browser()
        }));
        assert!(!is_fullscreen(&FullscreenFacts {
            own_process: true,
            ..browser()
        }));
        assert!(!is_fullscreen(&FullscreenFacts {
            maximized: true,
            ..browser()
        }));
        // The desktop and the taskbars cover their monitor by construction.
        for class in [
            "Progman",
            "WorkerW",
            "Shell_TrayWnd",
            "shell_secondarytraywnd",
        ] {
            assert!(
                !is_fullscreen(&FullscreenFacts {
                    class_name: class,
                    ..browser()
                }),
                "{class}"
            );
        }
        // A window that does not cover is not fullscreen either way.
        assert!(!is_fullscreen(&FullscreenFacts {
            covers: false,
            ..browser()
        }));
        assert!(!is_fullscreen(&FullscreenFacts::default()));
    }
}
