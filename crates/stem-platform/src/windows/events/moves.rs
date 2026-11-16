//! Window moves.
//!
//! Contract:
//! - Window moves: one more `SetWinEventHook`, for
//!   `EVENT_OBJECT_LOCATIONCHANGE` alone, whose callback keeps only
//!   `OBJID_WINDOW`/`CHILDID_SELF` reports about top-level windows
//!   (`rules::events::location_change_tracked`) and posts. The loop collects
//!   the windows it already tracks (`known`) into a small set and, once per
//!   [`LOCATION_SETTLE`] (150 ms, a one-shot [`ThreadTimer`] armed by the
//!   first report of a burst), re-queries each of them
//!   (`enumerate::query_moved_window`, which reuses the desktop answer
//!   last reported instead of asking the shell) and emits
//!   `WindowUpdated` only when its facts differ from the last report (the
//!   frame included), so a window being dragged costs the engine at most
//!   about seven updates a second and a window that is not tracked costs
//!   nothing here (the re-checks handle newcomers). The engine turns
//!   the frames into `DockState::overlapped`.

use std::time::Duration;

use stem_core::WindowId;
use windows::Win32::Foundation::HWND;

use super::state::ThreadState;
use super::timer::ThreadTimer;
use super::tracking::Report;
use crate::windows::util;

/// How long after the first location report of a burst the moved windows
/// are re-queried: a window being dragged is reported to the engine
/// at most once per this interval.
pub const LOCATION_SETTLE: Duration = Duration::from_millis(150);

impl ThreadState {
    /// A top-level window moved or resized: a tracked one is
    /// re-queried at the next flush; the foreground one also re-evaluates
    /// fullscreen. Untracked windows cost nothing here.
    pub(super) fn on_location_change(&mut self, hwnd: HWND) {
        if hwnd.0 as isize == self.foreground_hwnd {
            self.schedule_fullscreen_check();
        }
        let id = util::window_id(hwnd);
        if !self.known.contains_key(&id) {
            return;
        }
        if self.moved.insert(id) && self.moved.len() == 1 {
            log::trace!("window {:#x} is moving; flush in {LOCATION_SETTLE:?}", id.0);
        }
        self.schedule_location_flush();
    }

    /// Make sure the moved windows are re-queried [`LOCATION_SETTLE`] from
    /// now. A timer already armed is kept, so a burst costs one query per
    /// window; when no timer can be had the flush runs at once rather than
    /// never.
    fn schedule_location_flush(&mut self) {
        if self.location_timer.is_some() {
            return;
        }
        match ThreadTimer::start(LOCATION_SETTLE) {
            Some(timer) => self.location_timer = Some(timer),
            None => self.flush_moved(),
        }
    }

    /// The location timer fired: re-query every window that moved since the
    /// last flush and is still tracked, and report the ones whose facts
    /// changed (`WindowUpdated`) or that no longer qualify (`WindowRemoved`).
    pub(super) fn flush_moved(&mut self) {
        // Thread timers repeat; this one is one-shot.
        self.location_timer = None;
        let mut ids: Vec<WindowId> = self.moved.drain().collect();
        ids.sort_unstable();
        let mut updated = 0usize;
        let mut removed = 0usize;
        for id in &ids {
            if !self.known.contains_key(id) {
                continue;
            }
            match self.query_window(util::hwnd(*id), true).1 {
                Report::Updated => updated += 1,
                Report::Removed => removed += 1,
                Report::Added | Report::Unchanged => {}
            }
        }
        if !ids.is_empty() {
            log::trace!(
                "{} moved window(s) re-queried: {updated} updated, {removed} no longer qualify",
                ids.len()
            );
        }
    }
}
