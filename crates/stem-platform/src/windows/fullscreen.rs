//! Fullscreen detection: the facts behind
//! `rules::fullscreen`, gathered about the foreground window.
//!
//! Contract:
//! - [`evaluate`] takes the foreground window (`GetForegroundWindow`; NULL
//!   while the foreground changes hands or the station is locked) and
//!   answers whether it owns the whole screen, with its class and title for
//!   the events thread's log line and, when it does, the monitor it owns
//!   (`monitors::monitor_for_window`), so the engine hides only that
//!   monitor's dock.
//! - The facts: `SHQueryUserNotificationState` (the shell's own verdict,
//!   `Unknown` when the call fails), the frame from
//!   `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` with `GetWindowRect`
//!   as the fallback ([`enumerate::window_frame`], also the source of
//!   `RawWindow::rect`), the monitor's full bounds from
//!   `MonitorFromWindow(MONITOR_DEFAULTTONEAREST)` + `GetMonitorInfoW`,
//!   `GetClassNameW`, `IsIconic`, `IsZoomed`, and the owning process against
//!   our own. The decision itself is `rules::fullscreen::is_fullscreen`.
//! - Nothing here sends a message to another process, so a hung foreground
//!   app cannot stall the events thread: the title is the cached caption
//!   ([`enumerate::window_title`]), and windows of our own process skip the
//!   title read altogether.
//! - No `unwrap`, no panic: a rectangle that cannot be read makes `covers`
//!   false (one debug line) and the shell's state still decides.
//!
//! Runs on the events thread (`windows::events`), which owns the timing: one
//! evaluation per burst of foreground or location events.

use stem_core::{MonitorId, Rect};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::Shell::{
    QUNS_ACCEPTS_NOTIFICATIONS, QUNS_APP, QUNS_BUSY, QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE,
    QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsZoomed};

use super::{enumerate, monitors, util};
use crate::rules::fullscreen::{self as rules, FullscreenFacts, NotificationState};

// The host-independent codes in `rules::fullscreen` mirror shellapi.h; prove
// them against the SDK here, where the `windows` crate is available.
const _: () = {
    assert!(rules::QUNS_NOT_PRESENT == QUNS_NOT_PRESENT.0);
    assert!(rules::QUNS_BUSY == QUNS_BUSY.0);
    assert!(rules::QUNS_RUNNING_D3D_FULL_SCREEN == QUNS_RUNNING_D3D_FULL_SCREEN.0);
    assert!(rules::QUNS_PRESENTATION_MODE == QUNS_PRESENTATION_MODE.0);
    assert!(rules::QUNS_ACCEPTS_NOTIFICATIONS == QUNS_ACCEPTS_NOTIFICATIONS.0);
    assert!(rules::QUNS_QUIET_TIME == QUNS_QUIET_TIME.0);
    assert!(rules::QUNS_APP == QUNS_APP.0);
};

/// The answer about the foreground window, with what the log line needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub fullscreen: bool,
    /// The monitor the window owns while `fullscreen`, from
    /// `MonitorFromWindow`; `None` when it could not be read, and always
    /// while not fullscreen (the engine would not use it).
    pub monitor: Option<MonitorId>,
    /// `GetClassNameW`; empty for a NULL foreground.
    pub class_name: String,
    /// The cached caption; empty for a NULL foreground and for our own windows.
    pub title: String,
}

/// Whether `foreground` owns the whole screen. A NULL handle is judged by
/// the shell's state alone (it is what `GetForegroundWindow` returns between
/// two foreground windows and on the lock screen).
pub fn evaluate(foreground: HWND) -> Verdict {
    let notification_state = notification_state();
    let present = !foreground.is_invalid();
    let own_process = present && util::is_own_process_window(foreground);
    let class_name = if present {
        enumerate::class_name(foreground)
    } else {
        String::new()
    };
    let title = if present && !own_process {
        enumerate::window_title(foreground)
    } else {
        String::new()
    };
    // SAFETY: IsIconic accepts any handle value, NULL included.
    let minimized = present && unsafe { IsIconic(foreground) }.as_bool();
    // SAFETY: IsZoomed accepts any handle value, NULL included.
    let maximized = present && unsafe { IsZoomed(foreground) }.as_bool();
    let covers = present && covers_its_monitor(foreground);
    let facts = FullscreenFacts {
        covers,
        notification_state,
        class_name: &class_name,
        own_process,
        minimized,
        maximized,
    };
    let fullscreen = rules::is_fullscreen(&facts);
    log::trace!(
        "fullscreen check of {:#x}: {facts:?} -> {fullscreen}",
        foreground.0 as isize
    );
    // A NULL foreground can be fullscreen by the shell's state alone; it has
    // no monitor, and the engine then takes the primary monitor's dock.
    let monitor = if fullscreen && present {
        monitors::monitor_for_window(foreground)
    } else {
        None
    };
    Verdict {
        fullscreen,
        monitor,
        class_name,
        title,
    }
}

/// The window's frame contains its monitor's full bounds
/// (`rules::fullscreen::covers_monitor`); `false` when either cannot be read.
fn covers_its_monitor(hwnd: HWND) -> bool {
    let Some(frame) = enumerate::window_frame(hwnd) else {
        log::debug!(
            "fullscreen check: no frame for window {:#x}; not covering",
            hwnd.0 as isize
        );
        return false;
    };
    let Some(bounds) = monitor_bounds(hwnd) else {
        log::debug!(
            "fullscreen check: no monitor for window {:#x}; not covering",
            hwnd.0 as isize
        );
        return false;
    };
    rules::covers_monitor(frame, bounds)
}

/// Full bounds (`rcMonitor`, not the work area) of the monitor nearest to
/// the window, in physical pixels of the virtual desktop.
fn monitor_bounds(hwnd: HWND) -> Option<Rect> {
    // SAFETY: MonitorFromWindow accepts any handle value; NEAREST never
    // returns NULL for a live window, and NULL is checked below.
    let handle = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if handle.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a live MONITORINFO whose cbSize announces its size,
    // as the API requires; the call writes nothing past it.
    let ok = unsafe { GetMonitorInfoW(handle, &mut info) };
    if !ok.as_bool() {
        return None;
    }
    let r = info.rcMonitor;
    Some(Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top))
}

/// What the shell says about the user's state right now.
fn notification_state() -> NotificationState {
    // SAFETY: no arguments; the result carries the state or the error.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => NotificationState::from_code(state.0),
        Err(e) => {
            log::debug!("SHQueryUserNotificationState failed: {e}");
            NotificationState::Unknown
        }
    }
}
