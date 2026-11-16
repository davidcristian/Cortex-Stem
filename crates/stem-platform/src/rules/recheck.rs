//! Re-checks for windows that were not ready when they appeared.
//!
//! A window's create/show events can arrive before the window can be judged.
//! Example: `Start-Process calc` while another app had the foreground; the
//! Store app's `ApplicationFrameWindow` was rejected as `NotVisible` at both
//! its create and its show event, nothing looked at it again, and it stayed
//! off the dock until it took the foreground. `windows::events` therefore
//! keeps a [`RecheckSchedule`] of such windows and queries them again on a
//! thread timer.
//!
//! Policy (everything here is plain data, unit-tested on every host):
//!
//! | Question | Answer |
//! |----------|--------|
//! | Which windows | One looked at for a show-type event (`EVENT_OBJECT_CREATE`, `EVENT_OBJECT_SHOW`, shell hook `HSHELL_WINDOWCREATED`) and rejected *only* for not being visible or for being cloaked (rules 3 `NotVisible` and 7 `Cloaked` of `rules::eligibility`), facts a starting app changes on its own, while every other rule passes ([`classify`] → [`Check::Transient`]). |
//! | When | [`DELAYS_MS`] after the rejection that scheduled it (250 ms, 1 s, 3 s, all measured from that rejection); after the last attempt the window is dropped. A further show event while it is pending does not restart the schedule. |
//! | Leaves early | Destroyed ([`RecheckSchedule::forget`]); found eligible, rejected for a final reason, or gone by *any* later query ([`RecheckSchedule::observe`]). A hide does not cancel: a starting app may hide and show its window again, and a re-check of a rejected window costs only a cheap probe (no process handle, no COM). |
//! | Bound | At most [`MAX_PENDING`] windows; scheduling one more drops the oldest, so a burst of hidden windows cannot grow the set. |
//!
//! Time is an opaque millisecond counter supplied by the caller, as for
//! `rules::events::TitleDebouncer`. The caller arms one thread timer for
//! [`RecheckSchedule::next_due`], and when it fires calls
//! [`RecheckSchedule::take_due`], queries each returned window and feeds the
//! result back through [`RecheckSchedule::observe`].

use std::collections::VecDeque;

use super::eligibility::{self, Rejection, WindowFacts};

/// Offsets of the re-checks from the rejection that scheduled them.
pub const DELAYS_MS: [u64; 3] = [250, 1_000, 3_000];
/// Number of re-checks per scheduled window.
pub const ATTEMPTS: u8 = DELAYS_MS.len() as u8;
/// Most windows pending at once; the oldest is dropped beyond that.
pub const MAX_PENDING: usize = 64;

/// How a looked-at window stands, as far as re-checks are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// A taskbar window.
    Eligible,
    /// Rejected only for `reason` (`NotVisible` or `Cloaked`): with that
    /// fact cleared, every rule would pass, so it may qualify later.
    Transient(Rejection),
    /// Rejected for `reason`, which a later look will not change by itself.
    /// For a window that is also hidden or cloaked, `reason` is the rule it
    /// would still fail once shown and uncloaked.
    Final(Rejection),
    /// The handle no longer names a window.
    Gone,
}

/// `true` for the rejections a window can outgrow by itself while it starts
/// (rule 3 `NotVisible`, rule 8 `Cloaked`); every other one is final.
pub fn is_transient(reason: Rejection) -> bool {
    matches!(reason, Rejection::NotVisible | Rejection::Cloaked)
}

/// Classify a live window's facts.
pub fn classify(facts: &WindowFacts) -> Check {
    match eligibility::evaluate(facts) {
        Ok(()) => Check::Eligible,
        Err(reason) if is_transient(reason) => {
            // Would it qualify once shown and uncloaked?
            let settled = WindowFacts {
                is_visible: true,
                cloak_flags: 0,
                ..facts.clone()
            };
            match eligibility::evaluate(&settled) {
                Ok(()) => Check::Transient(reason),
                Err(other) => Check::Final(other),
            }
        }
        Err(reason) => Check::Final(reason),
    }
}

/// One re-check that is due, as returned by [`RecheckSchedule::take_due`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    pub id: u64,
    /// 1-based, up to [`ATTEMPTS`].
    pub attempt: u8,
    /// No re-check follows: the schedule has already dropped the window, so
    /// a still-transient result ends here.
    pub is_last: bool,
    /// When the window was first rejected (the schedule's origin).
    pub since_ms: u64,
}

/// What [`RecheckSchedule::on_show`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheduled {
    /// Not pending afterwards (eligible, final, or gone).
    No,
    /// Already pending; its schedule is unchanged.
    AlreadyPending,
    /// Newly pending; `evicted` is the oldest window dropped to make room.
    Yes { evicted: Option<u64> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    id: u64,
    since_ms: u64,
    /// Re-checks already handed out by `take_due`.
    attempts: u8,
    due_ms: u64,
}

/// The pending re-checks, oldest first. See the module documentation.
#[derive(Debug, Clone, Default)]
pub struct RecheckSchedule {
    pending: VecDeque<Entry>,
}

impl RecheckSchedule {
    /// A show-type event's query of `id` at `now_ms` gave `check`. A
    /// transient rejection schedules the window (unless it is pending
    /// already); anything else takes it out of the schedule.
    pub fn on_show(&mut self, id: u64, check: Check, now_ms: u64) -> Scheduled {
        if !matches!(check, Check::Transient(_)) {
            self.forget(id);
            return Scheduled::No;
        }
        if self.contains(id) {
            return Scheduled::AlreadyPending;
        }
        let evicted = if self.pending.len() >= MAX_PENDING {
            self.pending.pop_front().map(|e| e.id)
        } else {
            None
        };
        self.pending.push_back(Entry {
            id,
            since_ms: now_ms,
            attempts: 0,
            due_ms: now_ms.saturating_add(DELAYS_MS[0]),
        });
        Scheduled::Yes { evicted }
    }

    /// Any other query of `id` (an update, a title change, the foreground
    /// path, a re-check): eligible, final or gone takes the window out of
    /// the schedule; transient leaves it as it is.
    pub fn observe(&mut self, id: u64, check: Check) {
        if !matches!(check, Check::Transient(_)) {
            self.forget(id);
        }
    }

    /// Drop `id` (the window was destroyed). Returns whether it was pending.
    pub fn forget(&mut self, id: u64) -> bool {
        let before = self.pending.len();
        self.pending.retain(|e| e.id != id);
        self.pending.len() != before
    }

    /// Hand out every re-check due at `now_ms`, oldest window first. Each
    /// returned window is moved to its next attempt, or dropped when this
    /// was its last one, so a re-check is handed out exactly once even if the
    /// caller never reports back.
    pub fn take_due(&mut self, now_ms: u64) -> Vec<Due> {
        let mut due = Vec::new();
        self.pending.retain_mut(|e| {
            if e.due_ms > now_ms {
                return true;
            }
            e.attempts = e.attempts.saturating_add(1);
            let next = DELAYS_MS.get(usize::from(e.attempts)).copied();
            due.push(Due {
                id: e.id,
                attempt: e.attempts,
                is_last: next.is_none(),
                since_ms: e.since_ms,
            });
            match next {
                Some(delay) => {
                    e.due_ms = e.since_ms.saturating_add(delay);
                    true
                }
                None => false,
            }
        });
        due
    }

    /// When the earliest pending re-check is due, if any.
    pub fn next_due(&self) -> Option<u64> {
        self.pending.iter().map(|e| e.due_ms).min()
    }

    /// When `id` was first rejected, if it is pending.
    pub fn pending_since(&self, id: u64) -> Option<u64> {
        self.pending.iter().find(|e| e.id == id).map(|e| e.since_ms)
    }

    pub fn contains(&self, id: u64) -> bool {
        self.pending.iter().any(|e| e.id == id)
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests;
