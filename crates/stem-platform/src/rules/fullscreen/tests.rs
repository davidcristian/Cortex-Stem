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
