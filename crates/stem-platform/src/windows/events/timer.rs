//! The thread timer guard of the events thread.

use std::time::Duration;

use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

/// A thread timer (`SetTimer` with no window); `WM_TIMER` arrives as a
/// thread message with `wParam` = the id. Killed on drop.
pub(super) struct ThreadTimer(usize);

impl ThreadTimer {
    pub(super) fn start(interval: Duration) -> Option<Self> {
        let millis = u32::try_from(interval.as_millis())
            .unwrap_or(u32::MAX)
            .max(1);
        // SAFETY: a NULL window and NULL callback create a thread timer whose
        // messages are delivered to this thread's queue.
        let id = unsafe { SetTimer(None, 0, millis, None) };
        if id == 0 {
            log::warn!("SetTimer({millis} ms) failed");
            return None;
        }
        Some(Self(id))
    }

    pub(super) fn id(&self) -> usize {
        self.0
    }
}

impl Drop for ThreadTimer {
    fn drop(&mut self) {
        // SAFETY: the id came from SetTimer(None, ..) on this thread.
        if let Err(e) = unsafe { KillTimer(None, self.0) } {
            log::debug!("KillTimer failed: {e}");
        }
    }
}
