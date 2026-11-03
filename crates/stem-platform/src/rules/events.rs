//! WinEvent / shell-hook translation table.
//!
//! `windows::events` installs hooks for exactly the ranges in
//! [`HOOK_RANGES`], filters each callback with [`win_event_intent`], and acts
//! on the resulting [`Intent`]. The shell hook window's messages go through
//! [`shell_hook_intent`]. Title-change storms are coalesced per window with
//! [`TitleDebouncer`]. Everything here is plain data so it is unit-tested on
//! every host.
//!
//! Translation table (the callback additionally requires
//! `idObject == OBJID_WINDOW`, `idChild == CHILDID_SELF` and a top-level
//! window; see [`win_event_intent`]):
//!
//! | Source     | Event                                                     | Intent         | Emitted as |
//! |------------|-----------------------------------------------------------|----------------|------------|
//! | WinEvent   | `EVENT_OBJECT_CREATE`, `EVENT_OBJECT_SHOW`                | `Added`        | `WindowAdded` when `query_window` says eligible (`WindowUpdated` if already known) |
//! | WinEvent   | `EVENT_OBJECT_DESTROY`, `EVENT_OBJECT_HIDE`               | `Removed`      | `WindowRemoved` when the window was known |
//! | WinEvent   | `EVENT_OBJECT_NAMECHANGE`                                 | `TitleChanged` | `WindowUpdated` after the [`TitleDebouncer`] window (≤ 50 ms) |
//! | WinEvent   | `EVENT_SYSTEM_MINIMIZESTART`, `EVENT_SYSTEM_MINIMIZEEND`, `EVENT_OBJECT_CLOAKED`, `EVENT_OBJECT_UNCLOAKED` | `Updated` | `WindowUpdated` (or `WindowRemoved` when no longer eligible) |
//! | WinEvent   | `EVENT_SYSTEM_FOREGROUND`                                 | `Foreground`   | `ForegroundChanged` |
//! | Shell hook | `HSHELL_WINDOWACTIVATED`, `HSHELL_RUDEAPPACTIVATED`       | `Foreground`   | `ForegroundChanged` |
//! | Shell hook | `HSHELL_FLASH`                                            | `Attention`    | `WindowAttention { flashing: true }` |
//! | any        | anything else                                             | `Ignore`       | — |
//!
//! The constants mirror `winuser.h`; `windows/events.rs` asserts at compile
//! time that they equal the SDK values from the `windows` crate.

use std::collections::BTreeMap;

pub const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
pub const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
pub const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;
pub const EVENT_OBJECT_CREATE: u32 = 0x8000;
pub const EVENT_OBJECT_DESTROY: u32 = 0x8001;
pub const EVENT_OBJECT_SHOW: u32 = 0x8002;
pub const EVENT_OBJECT_HIDE: u32 = 0x8003;
pub const EVENT_OBJECT_NAMECHANGE: u32 = 0x800C;
pub const EVENT_OBJECT_CLOAKED: u32 = 0x8017;
pub const EVENT_OBJECT_UNCLOAKED: u32 = 0x8018;

/// `idObject` value for the window itself.
pub const OBJID_WINDOW: i32 = 0;
/// `idChild` value for the object itself (not one of its children).
pub const CHILDID_SELF: i32 = 0;

pub const HSHELL_WINDOWACTIVATED: u32 = 4;
pub const HSHELL_REDRAW: u32 = 6;
/// Set on `HSHELL_RUDEAPPACTIVATED` and `HSHELL_FLASH`.
pub const HSHELL_HIGHBIT: u32 = 0x8000;
pub const HSHELL_RUDEAPPACTIVATED: u32 = HSHELL_WINDOWACTIVATED | HSHELL_HIGHBIT;
pub const HSHELL_FLASH: u32 = HSHELL_REDRAW | HSHELL_HIGHBIT;

/// `(eventMin, eventMax)` pairs for `SetWinEventHook`, covering exactly the
/// events in the translation table and nothing else.
pub const HOOK_RANGES: &[(u32, u32)] = &[
    (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
    (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
    (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
    (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
    (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
];

/// What the event thread should do about an OS event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Query the window; emit `WindowAdded` if it qualifies.
    Added,
    /// Emit `WindowRemoved` if the window was known.
    Removed,
    /// Re-query the window; emit `WindowUpdated` (or `WindowRemoved`).
    Updated,
    /// Like `Updated`, but coalesced through the [`TitleDebouncer`].
    TitleChanged,
    /// Emit `ForegroundChanged`.
    Foreground,
    /// Emit `WindowAttention`.
    Attention,
    /// Nothing to do.
    Ignore,
}

/// Whether the hook callback could establish that the window is top-level
/// (`GetAncestor(GA_ROOT) == hwnd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopLevel {
    Yes,
    No,
    /// The window is already gone (`GetAncestor` returned NULL), which is
    /// normal for `EVENT_OBJECT_DESTROY` delivered out of context.
    Unknown,
}

/// The event-id column of the translation table, without the filters.
pub fn event_intent(event: u32) -> Intent {
    match event {
        EVENT_OBJECT_CREATE | EVENT_OBJECT_SHOW => Intent::Added,
        EVENT_OBJECT_DESTROY | EVENT_OBJECT_HIDE => Intent::Removed,
        EVENT_OBJECT_NAMECHANGE => Intent::TitleChanged,
        EVENT_SYSTEM_MINIMIZESTART
        | EVENT_SYSTEM_MINIMIZEEND
        | EVENT_OBJECT_CLOAKED
        | EVENT_OBJECT_UNCLOAKED => Intent::Updated,
        EVENT_SYSTEM_FOREGROUND => Intent::Foreground,
        _ => Intent::Ignore,
    }
}

/// Filter + translate a `WINEVENTPROC` callback. Returns [`Intent::Ignore`]
/// for anything that is not about a top-level window as a whole. A window
/// whose top-level status is [`TopLevel::Unknown`] is accepted only for
/// removals: it has already been destroyed, so it can only ever be forgotten.
pub fn win_event_intent(event: u32, id_object: i32, id_child: i32, top_level: TopLevel) -> Intent {
    if id_object != OBJID_WINDOW || id_child != CHILDID_SELF {
        return Intent::Ignore;
    }
    let intent = event_intent(event);
    match top_level {
        TopLevel::Yes => intent,
        TopLevel::Unknown if intent == Intent::Removed => intent,
        TopLevel::No | TopLevel::Unknown => Intent::Ignore,
    }
}

/// Translate the `wParam` of a `RegisterShellHookWindow` message.
pub fn shell_hook_intent(code: u32) -> Intent {
    match code {
        HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED => Intent::Foreground,
        HSHELL_FLASH => Intent::Attention,
        _ => Intent::Ignore,
    }
}

/// Coalesces `EVENT_OBJECT_NAMECHANGE` storms per window.
///
/// Policy: the first change for a window schedules one flush
/// [`TitleDebouncer::WINDOW_MS`] later; further changes inside that window
/// are absorbed. A window whose title changes continuously therefore yields
/// at most one `WindowUpdated` per 50 ms, and a single change is reported
/// within 50 ms. Time is an opaque millisecond counter supplied by the caller.
#[derive(Debug, Clone, Default)]
pub struct TitleDebouncer {
    /// window id -> flush due time (ms).
    pending: BTreeMap<u64, u64>,
}

impl TitleDebouncer {
    pub const WINDOW_MS: u64 = 50;

    /// Record a title change for `id` at `now_ms`. Returns `true` when this
    /// change scheduled a new flush (the caller should make sure a timer will
    /// call [`take_due`](Self::take_due) at [`next_due`](Self::next_due)).
    pub fn record(&mut self, id: u64, now_ms: u64) -> bool {
        if self.pending.contains_key(&id) {
            return false;
        }
        self.pending.insert(id, now_ms + Self::WINDOW_MS);
        true
    }

    /// Remove and return every window whose flush is due at `now_ms`.
    pub fn take_due(&mut self, now_ms: u64) -> Vec<u64> {
        let due: Vec<u64> = self
            .pending
            .iter()
            .filter(|&(_, &at)| at <= now_ms)
            .map(|(&id, _)| id)
            .collect();
        for id in &due {
            self.pending.remove(id);
        }
        due
    }

    /// Earliest pending flush time, if any.
    pub fn next_due(&self) -> Option<u64> {
        self.pending.values().copied().min()
    }

    /// Drop a pending change (the window went away).
    pub fn forget(&mut self, id: u64) {
        self.pending.remove(&id);
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn event_table_matches_the_contract() {
        assert_eq!(event_intent(EVENT_OBJECT_CREATE), Intent::Added);
        assert_eq!(event_intent(EVENT_OBJECT_SHOW), Intent::Added);
        assert_eq!(event_intent(EVENT_OBJECT_DESTROY), Intent::Removed);
        assert_eq!(event_intent(EVENT_OBJECT_HIDE), Intent::Removed);
        assert_eq!(event_intent(EVENT_OBJECT_NAMECHANGE), Intent::TitleChanged);
        assert_eq!(event_intent(EVENT_SYSTEM_MINIMIZESTART), Intent::Updated);
        assert_eq!(event_intent(EVENT_SYSTEM_MINIMIZEEND), Intent::Updated);
        assert_eq!(event_intent(EVENT_OBJECT_CLOAKED), Intent::Updated);
        assert_eq!(event_intent(EVENT_OBJECT_UNCLOAKED), Intent::Updated);
        assert_eq!(event_intent(EVENT_SYSTEM_FOREGROUND), Intent::Foreground);
        // EVENT_OBJECT_LOCATIONCHANGE and friends are never hooked, but a
        // stray id must still be harmless.
        assert_eq!(event_intent(0x800B), Intent::Ignore);
        assert_eq!(event_intent(0), Intent::Ignore);
    }

    #[test]
    fn hook_ranges_cover_exactly_the_table() {
        let mut hooked = Vec::new();
        for &(min, max) in HOOK_RANGES {
            assert!(min <= max);
            for event in min..=max {
                assert_ne!(
                    event_intent(event),
                    Intent::Ignore,
                    "hooked event {event:#06x} has no intent"
                );
                hooked.push(event);
            }
        }
        hooked.sort_unstable();
        hooked.dedup();
        assert_eq!(
            hooked,
            vec![
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_MINIMIZESTART,
                EVENT_SYSTEM_MINIMIZEEND,
                EVENT_OBJECT_CREATE,
                EVENT_OBJECT_DESTROY,
                EVENT_OBJECT_SHOW,
                EVENT_OBJECT_HIDE,
                EVENT_OBJECT_NAMECHANGE,
                EVENT_OBJECT_CLOAKED,
                EVENT_OBJECT_UNCLOAKED,
            ]
        );
    }

    #[test]
    fn callback_filter_rejects_children_and_sub_objects() {
        let ev = EVENT_OBJECT_SHOW;
        assert_eq!(
            win_event_intent(ev, OBJID_WINDOW, CHILDID_SELF, TopLevel::Yes),
            Intent::Added
        );
        // OBJID_CLIENT (-4), OBJID_CARET (-8), ... are not the window.
        assert_eq!(
            win_event_intent(ev, -4, CHILDID_SELF, TopLevel::Yes),
            Intent::Ignore
        );
        // A child id means a list item / menu entry, not the window.
        assert_eq!(
            win_event_intent(ev, OBJID_WINDOW, 3, TopLevel::Yes),
            Intent::Ignore
        );
        // Child windows (controls) are not top-level.
        assert_eq!(
            win_event_intent(ev, OBJID_WINDOW, CHILDID_SELF, TopLevel::No),
            Intent::Ignore
        );
    }

    #[test]
    fn destroyed_windows_are_forgotten_even_when_already_gone() {
        assert_eq!(
            win_event_intent(
                EVENT_OBJECT_DESTROY,
                OBJID_WINDOW,
                CHILDID_SELF,
                TopLevel::Unknown
            ),
            Intent::Removed
        );
        assert_eq!(
            win_event_intent(
                EVENT_OBJECT_HIDE,
                OBJID_WINDOW,
                CHILDID_SELF,
                TopLevel::Unknown
            ),
            Intent::Removed
        );
        // ...but nothing else is accepted for a window we cannot inspect.
        for ev in [
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_NAMECHANGE,
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_MINIMIZESTART,
        ] {
            assert_eq!(
                win_event_intent(ev, OBJID_WINDOW, CHILDID_SELF, TopLevel::Unknown),
                Intent::Ignore,
                "{ev:#06x}"
            );
        }
    }

    #[test]
    fn shell_hook_codes() {
        assert_eq!(HSHELL_RUDEAPPACTIVATED, 0x8004);
        assert_eq!(HSHELL_FLASH, 0x8006);
        assert_eq!(
            shell_hook_intent(HSHELL_WINDOWACTIVATED),
            Intent::Foreground
        );
        assert_eq!(
            shell_hook_intent(HSHELL_RUDEAPPACTIVATED),
            Intent::Foreground
        );
        assert_eq!(shell_hook_intent(HSHELL_FLASH), Intent::Attention);
        // Plain HSHELL_REDRAW (title/icon repaint) is not a flash.
        assert_eq!(shell_hook_intent(HSHELL_REDRAW), Intent::Ignore);
        // HSHELL_WINDOWCREATED / DESTROYED are covered by WinEvents.
        assert_eq!(shell_hook_intent(1), Intent::Ignore);
        assert_eq!(shell_hook_intent(2), Intent::Ignore);
        assert_eq!(shell_hook_intent(0), Intent::Ignore);
    }

    #[test]
    fn debouncer_coalesces_per_window_and_flushes_once() {
        let mut d = TitleDebouncer::default();
        assert!(d.is_empty());
        assert_eq!(d.next_due(), None);

        assert!(d.record(1, 100));
        assert_eq!(d.next_due(), Some(150));
        // Storm inside the window: absorbed.
        assert!(!d.record(1, 110));
        assert!(!d.record(1, 149));
        assert_eq!(d.next_due(), Some(150));
        // Another window has its own timer.
        assert!(d.record(2, 120));
        assert_eq!(d.next_due(), Some(150));

        assert_eq!(d.take_due(149), Vec::<u64>::new());
        assert_eq!(d.take_due(150), vec![1]);
        assert_eq!(d.next_due(), Some(170));
        assert_eq!(d.take_due(200), vec![2]);
        assert!(d.is_empty());

        // After a flush the next change starts a new window.
        assert!(d.record(1, 200));
        assert_eq!(d.next_due(), Some(250));
    }

    #[test]
    fn debouncer_bounds_the_rate_for_a_continuously_changing_title() {
        let mut d = TitleDebouncer::default();
        let mut flushes = 0;
        let mut now = 0;
        while now < 1000 {
            d.record(7, now);
            if !d.take_due(now).is_empty() {
                flushes += 1;
            }
            now += 5;
        }
        // A flush at t=50 closes the window; the next change (t=55) opens a
        // new one, so flushes happen every 55 ms: at 50, 105, ..., 985.
        assert_eq!(flushes, 18);
        assert!(flushes <= 1000 / TitleDebouncer::WINDOW_MS);
    }

    #[test]
    fn forget_drops_a_pending_change() {
        let mut d = TitleDebouncer::default();
        d.record(5, 0);
        d.forget(5);
        assert!(d.is_empty());
        assert_eq!(d.take_due(1000), Vec::<u64>::new());
        // Forgetting an unknown id is a no-op.
        d.forget(6);
    }
}
