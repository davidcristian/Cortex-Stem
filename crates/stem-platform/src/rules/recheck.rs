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
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::rules::eligibility::{CLOAKED_APP, CLOAKED_SHELL, Owner, UWP_FRAME_CLASS};

    const OWN_PID: u32 = 4242;
    const NOT_VISIBLE: Check = Check::Transient(Rejection::NotVisible);

    fn notepad() -> WindowFacts {
        WindowFacts {
            process_id: 1000,
            own_process_id: OWN_PID,
            is_root: true,
            is_visible: true,
            class_name: "Notepad".into(),
            title: "Untitled - Notepad".into(),
            is_tool_window: false,
            is_app_window: false,
            is_no_activate: false,
            owner: Owner::None,
            cloak_flags: 0,
        }
    }

    /// The finding: a Store app's frame before it is shown (no title yet).
    fn hidden_calculator_frame() -> WindowFacts {
        WindowFacts {
            class_name: UWP_FRAME_CLASS.into(),
            title: String::new(),
            is_visible: false,
            ..notepad()
        }
    }

    fn hidden(facts: WindowFacts) -> WindowFacts {
        WindowFacts {
            is_visible: false,
            ..facts
        }
    }

    #[test]
    fn only_not_visible_and_cloaked_are_transient() {
        let all = [
            Rejection::OwnProcess,
            Rejection::NotRoot,
            Rejection::NotVisible,
            Rejection::ShellClass,
            Rejection::ToolWindow,
            Rejection::NoActivate,
            Rejection::Owned,
            Rejection::Cloaked,
            Rejection::EmptyTitle,
        ];
        let transient: Vec<Rejection> = all.into_iter().filter(|&r| is_transient(r)).collect();
        assert_eq!(transient, vec![Rejection::NotVisible, Rejection::Cloaked]);
    }

    #[test]
    fn classify_eligible_and_transient_windows() {
        assert_eq!(classify(&notepad()), Check::Eligible);
        assert_eq!(classify(&hidden(notepad())), NOT_VISIBLE);
        assert_eq!(classify(&hidden_calculator_frame()), NOT_VISIBLE);
        let app_cloaked = WindowFacts {
            cloak_flags: CLOAKED_APP,
            ..notepad()
        };
        assert_eq!(classify(&app_cloaked), Check::Transient(Rejection::Cloaked));
        // Hidden and cloaked: the first rule that fails is the reason.
        assert_eq!(classify(&hidden(app_cloaked)), NOT_VISIBLE);
        // Shell-cloaked (other virtual desktop) windows are eligible.
        let other_desktop = WindowFacts {
            cloak_flags: CLOAKED_SHELL,
            ..notepad()
        };
        assert_eq!(classify(&other_desktop), Check::Eligible);
    }

    #[test]
    fn classify_hidden_windows_that_would_fail_anyway_as_final() {
        let tool = WindowFacts {
            is_tool_window: true,
            ..notepad()
        };
        assert_eq!(classify(&hidden(tool)), Check::Final(Rejection::ToolWindow));
        let untitled = WindowFacts {
            title: "  ".into(),
            ..notepad()
        };
        assert_eq!(
            classify(&hidden(untitled.clone())),
            Check::Final(Rejection::EmptyTitle)
        );
        let owned = WindowFacts {
            owner: Owner::Visible,
            ..notepad()
        };
        assert_eq!(classify(&hidden(owned)), Check::Final(Rejection::Owned));
        let core = WindowFacts {
            class_name: "Windows.UI.Core.CoreWindow".into(),
            ..notepad()
        };
        assert_eq!(classify(&hidden(core)), Check::Final(Rejection::ShellClass));
        // Cloaked by the app, and untitled once uncloaked.
        let cloaked_untitled = WindowFacts {
            cloak_flags: CLOAKED_APP,
            ..untitled
        };
        assert_eq!(
            classify(&cloaked_untitled),
            Check::Final(Rejection::EmptyTitle)
        );
    }

    #[test]
    fn classify_keeps_rules_that_come_before_visibility() {
        let ours = WindowFacts {
            process_id: OWN_PID,
            ..hidden(notepad())
        };
        assert_eq!(classify(&ours), Check::Final(Rejection::OwnProcess));
        let child = WindowFacts {
            is_root: false,
            ..hidden(notepad())
        };
        assert_eq!(classify(&child), Check::Final(Rejection::NotRoot));
    }

    #[test]
    fn a_transient_rejection_is_rechecked_at_250ms_1s_and_3s_then_dropped() {
        let mut s = RecheckSchedule::default();
        assert_eq!(s.next_due(), None);
        assert_eq!(
            s.on_show(7, NOT_VISIBLE, 100),
            Scheduled::Yes { evicted: None }
        );
        assert!(s.contains(7));
        assert_eq!(s.pending_since(7), Some(100));
        assert_eq!(s.next_due(), Some(350));

        assert_eq!(s.take_due(349), vec![]);
        let due = |attempt, is_last| Due {
            id: 7,
            attempt,
            is_last,
            since_ms: 100,
        };
        assert_eq!(s.take_due(350), vec![due(1, false)]);
        // Handed out once: the next one is 1 s after the rejection.
        assert_eq!(s.take_due(350), vec![]);
        assert_eq!(s.next_due(), Some(1_100));
        s.observe(7, NOT_VISIBLE);
        assert_eq!(s.take_due(1_100), vec![due(2, false)]);
        s.observe(7, NOT_VISIBLE);
        assert_eq!(s.next_due(), Some(3_100));
        assert_eq!(s.take_due(3_099), vec![]);
        assert_eq!(s.take_due(3_100), vec![due(3, true)]);
        // The last attempt already dropped it.
        assert!(s.is_empty());
        assert_eq!(s.next_due(), None);
        assert_eq!(s.take_due(u64::MAX), vec![]);
        assert_eq!(usize::from(ATTEMPTS), DELAYS_MS.len());
    }

    #[test]
    fn a_late_timer_hands_out_one_attempt_per_call() {
        let mut s = RecheckSchedule::default();
        s.on_show(1, NOT_VISIBLE, 0);
        // The timer fired 5 s late: all three are overdue, one per call.
        let attempts: Vec<u8> = (0..4)
            .flat_map(|_| s.take_due(5_000))
            .map(|d| d.attempt)
            .collect();
        assert_eq!(attempts, vec![1, 2, 3]);
        assert!(s.is_empty());
    }

    #[test]
    fn a_second_show_while_pending_does_not_restart_the_schedule() {
        let mut s = RecheckSchedule::default();
        s.on_show(7, NOT_VISIBLE, 0);
        assert_eq!(s.on_show(7, NOT_VISIBLE, 200), Scheduled::AlreadyPending);
        assert_eq!(
            s.on_show(7, Check::Transient(Rejection::Cloaked), 220),
            Scheduled::AlreadyPending
        );
        assert_eq!(s.next_due(), Some(250));
        assert_eq!(s.pending_since(7), Some(0));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn only_transient_rejections_are_scheduled() {
        let mut s = RecheckSchedule::default();
        for check in [
            Check::Eligible,
            Check::Final(Rejection::ToolWindow),
            Check::Final(Rejection::NotRoot),
            Check::Gone,
        ] {
            assert_eq!(s.on_show(1, check, 0), Scheduled::No, "{check:?}");
        }
        assert!(s.is_empty());
        assert_eq!(s.next_due(), None);
    }

    #[test]
    fn eligible_final_or_gone_from_any_query_leaves_the_schedule() {
        for (check, stays) in [
            (Check::Eligible, false),
            (Check::Final(Rejection::Owned), false),
            (Check::Gone, false),
            (NOT_VISIBLE, true),
            (Check::Transient(Rejection::Cloaked), true),
        ] {
            let mut s = RecheckSchedule::default();
            s.on_show(1, NOT_VISIBLE, 0);
            s.on_show(2, NOT_VISIBLE, 10);
            s.observe(1, check);
            assert_eq!(s.contains(1), stays, "{check:?}");
            assert!(s.contains(2), "{check:?} touched another window");
        }
        // A show-type event that finds it eligible (e.g. HSHELL_WINDOWCREATED
        // after the WinEvents rejected it) resolves it too.
        let mut s = RecheckSchedule::default();
        s.on_show(1, NOT_VISIBLE, 0);
        assert_eq!(s.on_show(1, Check::Eligible, 40), Scheduled::No);
        assert!(s.is_empty());
    }

    #[test]
    fn destroyed_windows_are_forgotten() {
        let mut s = RecheckSchedule::default();
        s.on_show(1, NOT_VISIBLE, 0);
        s.on_show(2, NOT_VISIBLE, 500);
        assert!(s.forget(1));
        assert!(!s.forget(1));
        assert!(!s.forget(99));
        assert_eq!(s.next_due(), Some(750));
        assert_eq!(s.take_due(750).len(), 1);
    }

    #[test]
    fn next_due_is_the_earliest_across_windows() {
        let mut s = RecheckSchedule::default();
        s.on_show(1, NOT_VISIBLE, 0);
        // Window 1's first re-check moves it to 1 000.
        assert_eq!(s.take_due(250).len(), 1);
        assert_eq!(s.next_due(), Some(1_000));
        // A newer window can be due earlier than an older one.
        s.on_show(2, NOT_VISIBLE, 600);
        assert_eq!(s.next_due(), Some(850));
        let ids: Vec<u64> = s.take_due(1_000).iter().map(|d| d.id).collect();
        // Oldest first.
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn the_pending_set_is_bounded_and_drops_the_oldest() {
        let mut s = RecheckSchedule::default();
        let max = MAX_PENDING as u64;
        for id in 0..max {
            assert_eq!(
                s.on_show(id, NOT_VISIBLE, id),
                Scheduled::Yes { evicted: None }
            );
        }
        assert_eq!(s.len(), MAX_PENDING);
        // Re-showing a pending window evicts nothing.
        assert_eq!(s.on_show(5, NOT_VISIBLE, 900), Scheduled::AlreadyPending);
        assert_eq!(
            s.on_show(max, NOT_VISIBLE, 1_000),
            Scheduled::Yes { evicted: Some(0) }
        );
        assert_eq!(
            s.on_show(max + 1, NOT_VISIBLE, 1_001),
            Scheduled::Yes { evicted: Some(1) }
        );
        assert_eq!(s.len(), MAX_PENDING);
        assert!(!s.contains(0));
        assert!(!s.contains(1));
        assert!(s.contains(2));
        assert!(s.contains(max + 1));
        // A burst of 1000 hidden windows leaves only the newest 64 pending.
        for id in 10_000..11_000 {
            s.on_show(id, NOT_VISIBLE, 2_000);
        }
        assert_eq!(s.len(), MAX_PENDING);
        assert!(s.contains(10_999));
        assert!(s.contains(11_000 - max));
        assert!(!s.contains(11_000 - max - 1));
    }
}
