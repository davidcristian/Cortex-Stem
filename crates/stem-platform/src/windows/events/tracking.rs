//! The windows reported to the engine: queries, additions, updates and
//! removals, titles, the foreground window and snapshots.

use std::time::Duration;

use stem_core::{PlatformEvent, RawWindow, WindowId};
use windows::Win32::Foundation::HWND;

use super::state::ThreadState;
use super::timer::ThreadTimer;
use crate::rules::events::TitleDebouncer;
use crate::rules::recheck::Check;
use crate::windows::{enumerate, util};

/// What a successful or failed query of one window was reported as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Report {
    /// `WindowAdded`: the window was not tracked before.
    Added,
    /// `WindowUpdated`.
    Updated,
    /// Nothing: the facts equal the last report (only on the move path).
    Unchanged,
    /// `WindowRemoved` (or nothing, when it was not tracked): it no longer qualifies.
    Removed,
}

impl ThreadState {
    /// Re-query a window and report it as added, updated or removed, and
    /// resolve a pending re-check that the result settles. Returns how the
    /// window stands and whether this call added it.
    pub(super) fn query_and_report(&mut self, hwnd: HWND) -> (Check, bool) {
        let (check, report) = self.query_window(hwnd, false);
        (check, report == Report::Added)
    }

    /// The query behind [`Self::query_and_report`] and the move path:
    /// with `only_changes` a `WindowUpdated` goes out only when the
    /// facts differ from the last report, so a window dragged around costs
    /// the engine nothing while its frame is unchanged between two flushes,
    /// and a tracked window keeps the desktop answer last reported instead
    /// of asking the shell again (a move does not change the
    /// desktop, and the shell's call has no timeout).
    pub(super) fn query_window(&mut self, hwnd: HWND, only_changes: bool) -> (Check, Report) {
        let id = util::window_id(hwnd);
        let queried = match self.known.get(&id).filter(|_| only_changes) {
            Some(last) => enumerate::query_moved_window(hwnd, last.on_current_desktop),
            None => enumerate::query_window_checked(hwnd),
        };
        let (check, report) = match queried {
            Ok(raw) => (Check::Eligible, self.report_queried(id, raw, only_changes)),
            Err(check) => {
                if only_changes {
                    log::trace!("moved window {:#x} no longer qualifies: {check:?}", id.0);
                }
                self.remove_window(id);
                (check, Report::Removed)
            }
        };
        self.rechecks.observe(id.0, check);
        (check, report)
    }

    /// Emit what a successful query of `id` means: `WindowAdded` for a
    /// window not tracked yet, else `WindowUpdated`, unless `only_changes`
    /// and the facts equal the last report. `known` keeps the facts either
    /// way.
    fn report_queried(&mut self, id: WindowId, raw: RawWindow, only_changes: bool) -> Report {
        match self.known.get(&id) {
            None => {
                self.known.insert(id, raw.clone());
                self.emit(PlatformEvent::WindowAdded(raw));
                Report::Added
            }
            Some(previous) if only_changes && *previous == raw => Report::Unchanged,
            Some(_) => {
                if only_changes {
                    log::trace!(
                        "moved window {:#x}: WindowUpdated, frame {:?}",
                        id.0,
                        raw.rect
                    );
                }
                self.known.insert(id, raw.clone());
                self.emit(PlatformEvent::WindowUpdated(raw));
                Report::Updated
            }
        }
    }

    /// [`Self::query_and_report`] for an event about the window (`cause`
    /// names it for the log). A window the re-check schedule was waiting for
    /// that this call adds is logged as late.
    pub(super) fn refresh_window(&mut self, hwnd: HWND, cause: &str) -> Check {
        let id = util::window_id(hwnd);
        let pending_since = self.rechecks.pending_since(id.0);
        let (check, added) = self.query_and_report(hwnd);
        // One `added` line per `WindowAdded`, naming the source that won
        // (one per window, whichever source came first).
        if added {
            match pending_since {
                Some(since) => log::debug!(
                    "late window {:#x} added by {cause}, {} ms after it was first rejected",
                    id.0,
                    self.now_ms().saturating_sub(since)
                ),
                None => log::debug!("window {:#x} added by {cause}", id.0),
            }
        }
        check
    }

    pub(super) fn remove_window(&mut self, id: WindowId) {
        self.titles.forget(id.0);
        self.moved.remove(&id);
        if self.known.remove(&id).is_some() {
            // The pair of the `added` line; the trace line before it
            // names the event that caused it.
            log::debug!("window {:#x} removed", id.0);
            self.emit(PlatformEvent::WindowRemoved(id));
        }
    }

    pub(super) fn on_title_changed(&mut self, id: WindowId) {
        let now = self.now_ms();
        if self.titles.record(id.0, now) && self.title_timer.is_none() {
            self.title_timer = ThreadTimer::start(Duration::from_millis(TitleDebouncer::WINDOW_MS));
        }
    }

    pub(super) fn flush_titles(&mut self) {
        let now = self.now_ms();
        for id in self.titles.take_due(now) {
            self.refresh_window(util::hwnd(WindowId(id)), "EVENT_OBJECT_NAMECHANGE");
        }
        // Re-arm for the next pending flush, or stop the timer.
        self.title_timer = self.titles.next_due().and_then(|due| {
            ThreadTimer::start(Duration::from_millis(due.saturating_sub(now).max(1)))
        });
    }

    pub(super) fn emit_foreground(&mut self, hwnd: HWND) {
        // A foreground change is when a dock that has silently
        // dropped out of the topmost band gets covered; the check is posted.
        crate::windows::dock_window::assert_topmost_all();
        // Moves of this window re-evaluate fullscreen from now on, and
        // the verdict is taken once the switch has settled. Our own windows
        // count too: the settings window in front of a game ends fullscreen.
        self.foreground_hwnd = hwnd.0 as isize;
        self.schedule_fullscreen_check();
        if !hwnd.is_invalid() && util::is_own_process_window(hwnd) {
            // The dock (or the settings window) got the foreground: not an
            // app the user switched to, so the previous foreground stays.
            log::trace!(
                "foreground report for our own window {:#x} ignored",
                hwnd.0 as isize
            );
            return;
        }
        let id = (!hwnd.is_invalid() && enumerate::is_taskbar_candidate(hwnd))
            .then(|| util::window_id(hwnd));
        if let Some(id) = id
            && !self.known.contains_key(&id)
        {
            // A candidate we never saw shown (missed event): report it first
            // so the engine can mark it active.
            self.refresh_window(hwnd, "the foreground change");
        }
        self.emit(PlatformEvent::ForegroundChanged(id));
    }

    pub(super) fn emit_snapshot(&mut self) {
        // Whatever triggered this snapshot, it is the one a cloak burst was
        // waiting for.
        self.switch.reset();
        self.switch_timer = None;
        match enumerate::enumerate_taskbar_windows() {
            Ok(windows) => {
                self.known = windows.iter().map(|w| (w.id, w.clone())).collect();
                self.moved.clear();
                self.titles = TitleDebouncer::default();
                self.title_timer = None;
                // Listed now; windows still not ready stay pending.
                for w in &windows {
                    self.rechecks.forget(w.id.0);
                }
                self.emit(PlatformEvent::WindowsSnapshot(windows));
            }
            Err(e) => log::warn!("window enumeration failed: {e}"),
        }
    }
}
