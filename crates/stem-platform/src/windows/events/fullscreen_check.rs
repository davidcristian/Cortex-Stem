//! The fullscreen state of the foreground window.
//!
//! Contract:
//! - Fullscreen: the thread keeps the platform's verdict on whether
//!   the foreground window owns the whole screen (`fullscreen::evaluate`
//!   over `rules::fullscreen`) and emits `FullscreenChanged` when it
//!   changes, plus once right after the start snapshot. It is re-evaluated
//!   on every foreground report (WinEvent or shell hook, our own windows
//!   included), on a display change, and whenever the foreground window
//!   moves or resizes (the location reports of `moves` about the window this
//!   thread last saw in the foreground). Each trigger arms one
//!   [`FULLSCREEN_SETTLE`] timer (150 ms) if none is running, so a burst (F11
//!   in a browser fires dozens of location changes) costs one evaluation,
//!   against the live `GetForegroundWindow`. The verdict carries the
//!   window's monitor, so a fullscreen window that changes monitor
//!   is a change too. `info` on each change (`fullscreen on: <class>
//!   <title> on <monitor>` / `fullscreen off`).

use std::time::Duration;

use stem_core::PlatformEvent;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use super::state::ThreadState;
use super::timer::ThreadTimer;
use crate::windows::fullscreen;

/// How long after a foreground or location report the fullscreen verdict
/// is taken: one evaluation per burst, after the geometry settled.
pub const FULLSCREEN_SETTLE: Duration = Duration::from_millis(150);

impl ThreadState {
    /// Something that can change the fullscreen verdict happened:
    /// make sure one evaluation runs [`FULLSCREEN_SETTLE`] from now. A timer
    /// already armed is kept, so a burst costs one evaluation; when no timer
    /// can be had the evaluation runs at once rather than never.
    pub(super) fn schedule_fullscreen_check(&mut self) {
        if self.fullscreen_timer.is_some() {
            return;
        }
        match ThreadTimer::start(FULLSCREEN_SETTLE) {
            Some(timer) => self.fullscreen_timer = Some(timer),
            None => self.check_fullscreen(),
        }
    }

    /// Judge the live foreground window and emit `FullscreenChanged`
    /// when the verdict differs from the last one emitted, or when it is the
    /// first.
    pub(super) fn check_fullscreen(&mut self) {
        // Thread timers repeat; this one is one-shot.
        self.fullscreen_timer = None;
        // SAFETY: GetForegroundWindow has no preconditions; NULL is a valid answer.
        let foreground = unsafe { GetForegroundWindow() };
        self.foreground_hwnd = foreground.0 as isize;
        let verdict = fullscreen::evaluate(foreground);
        let next = (verdict.fullscreen, verdict.monitor);
        if self.fullscreen.as_ref() == Some(&next) {
            return;
        }
        let first = self.fullscreen.is_none();
        self.fullscreen = Some(next.clone());
        let (active, monitor) = next;
        if active {
            log::info!(
                "fullscreen on: {} {:?} on {}",
                verdict.class_name,
                verdict.title,
                monitor
                    .as_ref()
                    .map_or("an unknown monitor", |m| m.0.as_str())
            );
        } else if first {
            log::debug!("fullscreen off at start");
        } else {
            log::info!("fullscreen off");
        }
        self.emit(PlatformEvent::FullscreenChanged { active, monitor });
    }
}
