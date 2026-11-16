//! Re-checks of windows that were not ready when they were shown.
//!
//! Contract:
//! - Late-visible windows: a window that a create/show event or
//!   `HSHELL_WINDOWCREATED` finds hidden or cloaked, while it passes every
//!   other eligibility rule, is queried again 250 ms, 1 s and 3 s later on a
//!   thread timer (`rules::recheck`) and reported with the same
//!   `WindowAdded` a show produces once it qualifies. At most 64 windows are
//!   pending; one trace line per re-check, a debug line when a late window
//!   is added.
//!
//! **Re-checks.** Some windows are shown before they can be judged:
//! a Store app's `ApplicationFrameWindow` launched in the background was
//! seen rejected as `NotVisible` at both its create and its show event,
//! after which no event about it arrived until it took the foreground. The
//! schedule (which windows, when, when to give up, the 64-entry bound) is
//! `rules::recheck`; this thread keeps one [`ThreadTimer`] armed for the
//! earliest due re-check, and when it fires re-queries the due handles
//! through the same path a show takes (`query_and_report`). A destroy
//! (`EVENT_OBJECT_DESTROY`) forgets a pending window; any query that finds
//! it eligible, rejected for a final reason, or gone resolves it; a hide
//! does not cancel it. A snapshot resolves every window it lists.

use std::time::Duration;

use stem_core::WindowId;
use windows::Win32::Foundation::HWND;

use super::state::ThreadState;
use super::timer::ThreadTimer;
use crate::rules::recheck::{self, Check, Scheduled};
use crate::windows::util;

impl ThreadState {
    /// A show-type event (`EVENT_OBJECT_CREATE`, `EVENT_OBJECT_SHOW`,
    /// `HSHELL_WINDOWCREATED`): report the window if it qualifies; when it
    /// is only not ready yet (hidden or cloaked, every other rule passing),
    /// schedule its re-checks.
    pub(super) fn on_window_shown(&mut self, hwnd: HWND, cause: &str) {
        let id = util::window_id(hwnd);
        let check = self.refresh_window(hwnd, cause);
        let now = self.now_ms();
        if let Scheduled::Yes { evicted } = self.rechecks.on_show(id.0, check, now) {
            log::trace!(
                "window {:#x} not ready at {cause} ({check:?}); re-checks {:?} ms later",
                id.0,
                recheck::DELAYS_MS
            );
            if let Some(old) = evicted {
                log::trace!(
                    "re-check queue full ({} windows); dropped the oldest, {old:#x}",
                    recheck::MAX_PENDING
                );
            }
            self.arm_recheck_timer();
        }
    }

    /// The re-check timer fired: query every due window again, the same way
    /// a show does, and re-arm for the next one.
    pub(super) fn run_rechecks(&mut self) {
        // Thread timers repeat; stop this one, the next is armed below.
        self.recheck_timer = None;
        let now = self.now_ms();
        for due in self.rechecks.take_due(now) {
            let (check, added) = self.query_and_report(util::hwnd(WindowId(due.id)));
            let outcome = match check {
                Check::Eligible if added => "added",
                Check::Eligible => "already listed",
                Check::Transient(_) if due.is_last => "still not ready; giving up",
                Check::Transient(_) => "still not ready",
                Check::Final(_) => "will not qualify; dropped",
                Check::Gone => "gone; dropped",
            };
            log::trace!(
                "re-check {}/{} of window {:#x}: {check:?}, {outcome}",
                due.attempt,
                recheck::ATTEMPTS,
                due.id
            );
            if added {
                log::debug!(
                    "late window {:#x} added on re-check {}/{}, {} ms after it was first rejected",
                    due.id,
                    due.attempt,
                    recheck::ATTEMPTS,
                    now.saturating_sub(due.since_ms)
                );
            }
        }
        self.arm_recheck_timer();
    }

    /// Make sure a timer fires by the earliest pending re-check. A timer
    /// armed for an earlier time is kept: if what it was armed for has been
    /// resolved meanwhile, it finds nothing due and `run_rechecks` re-arms
    /// it or lets it stop.
    fn arm_recheck_timer(&mut self) {
        let Some(due) = self.rechecks.next_due() else {
            return;
        };
        if self
            .recheck_timer
            .as_ref()
            .is_some_and(|&(_, armed_for)| armed_for <= due)
        {
            return;
        }
        let wait = due.saturating_sub(self.now_ms()).max(1);
        self.recheck_timer =
            ThreadTimer::start(Duration::from_millis(wait)).map(|timer| (timer, due));
    }
}
