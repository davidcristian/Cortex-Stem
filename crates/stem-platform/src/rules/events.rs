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
//! | WinEvent   | `EVENT_OBJECT_CREATE`, `EVENT_OBJECT_SHOW`                | `Added`        | `WindowAdded` when `query_window_checked` says eligible (`WindowUpdated` if already known); re-checks when it is only not ready yet |
//! | WinEvent   | `EVENT_OBJECT_DESTROY`, `EVENT_OBJECT_HIDE`               | `Removed`      | `WindowRemoved` when the window was known; a destroy also drops its pending re-checks |
//! | WinEvent   | `EVENT_OBJECT_NAMECHANGE`                                 | `TitleChanged` | `WindowUpdated` after the [`TitleDebouncer`] window (≤ 50 ms) |
//! | WinEvent   | `EVENT_SYSTEM_MINIMIZESTART`, `EVENT_SYSTEM_MINIMIZEEND`, `EVENT_OBJECT_CLOAKED`, `EVENT_OBJECT_UNCLOAKED` | `Updated` | `WindowUpdated` (or `WindowRemoved` when no longer eligible); the two cloak events are also counted by the [`DesktopSwitchDetector`] |
//! | WinEvent   | `EVENT_SYSTEM_FOREGROUND`                                 | `Foreground`   | `ForegroundChanged` |
//! | Shell hook | `HSHELL_WINDOWCREATED`                                    | `Added`        | as for `EVENT_OBJECT_SHOW` |
//! | Shell hook | `HSHELL_WINDOWACTIVATED`, `HSHELL_RUDEAPPACTIVATED`       | `Foreground`   | `ForegroundChanged` |
//! | Shell hook | `HSHELL_FLASH`                                            | `Attention`    | `WindowAttention { flashing: true }` |
//! | Shell hook | `HSHELL_WINDOWDESTROYED`, `HSHELL_REDRAW`, every other code | `Ignore`     | — (a WinEvent already covers each; see `windows::events`, *Sources*); the message is still named by [`shell_hook_name`] in one trace line |
//! | any        | anything else                                             | `Ignore`       | — |
//!
//! `Added` from either source is a *show-type* event: when it finds the
//! window rejected only because it is not visible or cloaked yet, the window
//! is queried again later (`rules::recheck`). `HSHELL_WINDOWCREATED`
//! (documented for a new top-level, unowned window) reaches us on a path of
//! its own, so it is one more chance to judge a window whose create/show
//! WinEvents were evaluated too early.
//!
//! The constants mirror `winuser.h`; `windows/events.rs` asserts at compile
//! time that they equal the SDK values from the `windows` crate.

use std::collections::{BTreeMap, BTreeSet};

pub const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
pub const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
pub const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;
pub const EVENT_OBJECT_CREATE: u32 = 0x8000;
pub const EVENT_OBJECT_DESTROY: u32 = 0x8001;
pub const EVENT_OBJECT_SHOW: u32 = 0x8002;
pub const EVENT_OBJECT_HIDE: u32 = 0x8003;
/// Hooked on its own, with a callback that keeps only moves
/// and resizes of top-level windows ([`location_change_tracked`]) and a loop
/// that coalesces them; it is not in the translation table and never reaches
/// [`event_intent`].
pub const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
pub const EVENT_OBJECT_NAMECHANGE: u32 = 0x800C;
pub const EVENT_OBJECT_CLOAKED: u32 = 0x8017;
pub const EVENT_OBJECT_UNCLOAKED: u32 = 0x8018;

/// `idObject` value for the window itself.
pub const OBJID_WINDOW: i32 = 0;
/// `idChild` value for the object itself (not one of its children).
pub const CHILDID_SELF: i32 = 0;

pub const HSHELL_WINDOWCREATED: u32 = 1;
pub const HSHELL_WINDOWDESTROYED: u32 = 2;
pub const HSHELL_ACTIVATESHELLWINDOW: u32 = 3;
pub const HSHELL_WINDOWACTIVATED: u32 = 4;
pub const HSHELL_GETMINRECT: u32 = 5;
pub const HSHELL_REDRAW: u32 = 6;
pub const HSHELL_TASKMAN: u32 = 7;
pub const HSHELL_LANGUAGE: u32 = 8;
pub const HSHELL_SYSMENU: u32 = 9;
pub const HSHELL_ENDTASK: u32 = 10;
pub const HSHELL_ACCESSIBILITYSTATE: u32 = 11;
pub const HSHELL_APPCOMMAND: u32 = 12;
pub const HSHELL_WINDOWREPLACED: u32 = 13;
pub const HSHELL_WINDOWREPLACING: u32 = 14;
pub const HSHELL_MONITORCHANGED: u32 = 16;
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
    /// Query the window; emit `WindowAdded` if it qualifies. A show-type
    /// event: a transient rejection schedules re-checks (`rules::recheck`).
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

/// The name of a hooked event, for log lines.
pub fn event_name(event: u32) -> &'static str {
    match event {
        EVENT_SYSTEM_FOREGROUND => "EVENT_SYSTEM_FOREGROUND",
        EVENT_SYSTEM_MINIMIZESTART => "EVENT_SYSTEM_MINIMIZESTART",
        EVENT_SYSTEM_MINIMIZEEND => "EVENT_SYSTEM_MINIMIZEEND",
        EVENT_OBJECT_CREATE => "EVENT_OBJECT_CREATE",
        EVENT_OBJECT_DESTROY => "EVENT_OBJECT_DESTROY",
        EVENT_OBJECT_SHOW => "EVENT_OBJECT_SHOW",
        EVENT_OBJECT_HIDE => "EVENT_OBJECT_HIDE",
        EVENT_OBJECT_NAMECHANGE => "EVENT_OBJECT_NAMECHANGE",
        EVENT_OBJECT_CLOAKED => "EVENT_OBJECT_CLOAKED",
        EVENT_OBJECT_UNCLOAKED => "EVENT_OBJECT_UNCLOAKED",
        _ => "an unhooked event",
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

/// Filter of the `EVENT_OBJECT_LOCATIONCHANGE` hook's callback:
/// whether a report is about a top-level window as a whole moving or
/// resizing, and so worth a thread message. Caret and cursor moves
/// (`OBJID_CARET`, `OBJID_CURSOR`), child controls and a window that is
/// already gone are dropped in the callback, before they cost anything.
pub fn location_change_tracked(id_object: i32, id_child: i32, top_level: TopLevel) -> bool {
    id_object == OBJID_WINDOW && id_child == CHILDID_SELF && top_level == TopLevel::Yes
}

/// Translate the `wParam` of a `RegisterShellHookWindow` message.
pub fn shell_hook_intent(code: u32) -> Intent {
    match code {
        HSHELL_WINDOWCREATED => Intent::Added,
        HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED => Intent::Foreground,
        HSHELL_FLASH => Intent::Attention,
        _ => Intent::Ignore,
    }
}

/// The name of a shell hook code (every one `winuser.h` documents), for the
/// one trace line per message that lets a trace log be counted per source.
pub fn shell_hook_name(code: u32) -> &'static str {
    match code {
        HSHELL_WINDOWCREATED => "HSHELL_WINDOWCREATED",
        HSHELL_WINDOWDESTROYED => "HSHELL_WINDOWDESTROYED",
        HSHELL_ACTIVATESHELLWINDOW => "HSHELL_ACTIVATESHELLWINDOW",
        HSHELL_WINDOWACTIVATED => "HSHELL_WINDOWACTIVATED",
        HSHELL_GETMINRECT => "HSHELL_GETMINRECT",
        HSHELL_REDRAW => "HSHELL_REDRAW",
        HSHELL_TASKMAN => "HSHELL_TASKMAN",
        HSHELL_LANGUAGE => "HSHELL_LANGUAGE",
        HSHELL_SYSMENU => "HSHELL_SYSMENU",
        HSHELL_ENDTASK => "HSHELL_ENDTASK",
        HSHELL_ACCESSIBILITYSTATE => "HSHELL_ACCESSIBILITYSTATE",
        HSHELL_APPCOMMAND => "HSHELL_APPCOMMAND",
        HSHELL_WINDOWREPLACED => "HSHELL_WINDOWREPLACED",
        HSHELL_WINDOWREPLACING => "HSHELL_WINDOWREPLACING",
        HSHELL_MONITORCHANGED => "HSHELL_MONITORCHANGED",
        HSHELL_RUDEAPPACTIVATED => "HSHELL_RUDEAPPACTIVATED",
        HSHELL_FLASH => "HSHELL_FLASH",
        _ => "an unknown shell hook code",
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

/// Tells a virtual-desktop switch from the cloak events it causes.
///
/// There is no WinEvent for the switch itself. What arrives is the shell
/// cloaking every window of the desktop left and uncloaking every window
/// of the desktop entered, within a few milliseconds; each of those events
/// already re-queries its own window. A burst of them is the moment to
/// re-enumerate once, [`SNAPSHOT_DELAY_MS`](Self::SNAPSHOT_DELAY_MS) later
/// when the shell is done, so that a window whose event was missed, or
/// whose answer was taken mid-switch, is put right.
///
/// Policy: the [`BURST_COUNT`](Self::BURST_COUNT)th cloak or uncloak event
/// within [`BURST_WINDOW_MS`](Self::BURST_WINDOW_MS) of the first arms the
/// snapshot (one `true` from [`observe`](Self::observe)); further events
/// until the snapshot change nothing; the snapshot
/// [`reset`](Self::reset)s the detector. A lone cloak event (a window
/// moved to another desktop, a Store app suspending, the Start menu) arms
/// nothing: its own `WindowUpdated` is enough. Time is an opaque
/// millisecond counter supplied by the caller, as for [`TitleDebouncer`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopSwitchDetector {
    /// When the current burst started and how many events it holds.
    burst: Option<(u64, u32)>,
    armed: bool,
}

impl DesktopSwitchDetector {
    /// Two cloak events this close together are a burst.
    pub const BURST_WINDOW_MS: u64 = 100;
    /// How many cloak events make a burst.
    pub const BURST_COUNT: u32 = 2;
    /// How long after the burst was detected the snapshot is taken.
    pub const SNAPSHOT_DELAY_MS: u64 = 400;

    /// A cloak or uncloak event about a top-level window at `now_ms`.
    /// `true` once per burst: the caller arms a one-shot timer for
    /// [`SNAPSHOT_DELAY_MS`](Self::SNAPSHOT_DELAY_MS) and, when it fires,
    /// takes a snapshot and calls [`reset`](Self::reset).
    pub fn observe(&mut self, now_ms: u64) -> bool {
        if self.armed {
            return false;
        }
        let (start, count) = match self.burst {
            Some((start, count)) if now_ms.saturating_sub(start) <= Self::BURST_WINDOW_MS => {
                (start, count + 1)
            }
            _ => (now_ms, 1),
        };
        self.burst = Some((start, count));
        if count >= Self::BURST_COUNT {
            self.armed = true;
            return true;
        }
        false
    }

    /// A snapshot was taken, whatever triggered it: start over.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// A snapshot is pending.
    pub fn is_armed(&self) -> bool {
        self.armed
    }
}

/// Windows whose `IsWindowOnCurrentVirtualDesktop` refusal has been logged,
/// the way `rules::process_access::DeniedProcesses` remembers
/// processes: the shell is still asked on every query (an `HWND` is reused
/// once its window is destroyed, and a refusal can be transient), the line
/// is written once per window. Bounded by [`CAPACITY`](Self::CAPACITY);
/// full, the set starts over, so at worst a line repeats once per
/// `CAPACITY` further refusals.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopQueryFailures {
    logged: BTreeSet<u64>,
}

impl DesktopQueryFailures {
    /// Most window ids remembered at once.
    pub const CAPACITY: usize = 256;

    pub const fn new() -> Self {
        Self {
            logged: BTreeSet::new(),
        }
    }

    /// Window `id` refused the question. `true` when the refusal is new
    /// (log it); `false` when it was logged before.
    pub fn note(&mut self, id: u64) -> bool {
        if self.logged.contains(&id) {
            return false;
        }
        if self.logged.len() >= Self::CAPACITY {
            self.logged.clear();
        }
        self.logged.insert(id)
    }

    /// Window `id` answered: forget any refusal, so a refusal by the window
    /// that gets this handle next is logged again.
    pub fn clear(&mut self, id: u64) {
        self.logged.remove(&id);
    }

    pub fn len(&self) -> usize {
        self.logged.len()
    }

    pub fn is_empty(&self) -> bool {
        self.logged.is_empty()
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
        // EVENT_OBJECT_LOCATIONCHANGE has its own hook and callback;
        // in the table it, like any stray id, must be harmless.
        assert_eq!(EVENT_OBJECT_LOCATIONCHANGE, 0x800B);
        assert_eq!(event_intent(EVENT_OBJECT_LOCATIONCHANGE), Intent::Ignore);
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
        for &event in &hooked {
            assert!(event_name(event).starts_with("EVENT_"), "{event:#06x}");
        }
        assert_eq!(event_name(EVENT_OBJECT_LOCATIONCHANGE), "an unhooked event");
        assert!(!hooked.contains(&EVENT_OBJECT_LOCATIONCHANGE));
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
    fn location_changes_count_only_for_whole_top_level_windows() {
        assert!(location_change_tracked(
            OBJID_WINDOW,
            CHILDID_SELF,
            TopLevel::Yes
        ));
        // The cursor (-9) and the caret (-8) move with every mouse move and
        // keystroke; the client area (-4) is not the window.
        for object in [-9, -8, -4] {
            assert!(!location_change_tracked(
                object,
                CHILDID_SELF,
                TopLevel::Yes
            ));
        }
        assert!(!location_change_tracked(OBJID_WINDOW, 1, TopLevel::Yes));
        assert!(!location_change_tracked(
            OBJID_WINDOW,
            CHILDID_SELF,
            TopLevel::No
        ));
        // Nothing can be queried about a window that is already gone.
        assert!(!location_change_tracked(
            OBJID_WINDOW,
            CHILDID_SELF,
            TopLevel::Unknown
        ));
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
        // A new taskbar window is a show-type event.
        assert_eq!(HSHELL_WINDOWCREATED, 1);
        assert_eq!(shell_hook_intent(HSHELL_WINDOWCREATED), Intent::Added);
        // HSHELL_WINDOWDESTROYED is covered by EVENT_OBJECT_DESTROY.
        assert_eq!(HSHELL_WINDOWDESTROYED, 2);
        assert_eq!(shell_hook_intent(HSHELL_WINDOWDESTROYED), Intent::Ignore);
        assert_eq!(shell_hook_intent(0), Intent::Ignore);
    }

    /// Every code `winuser.h` documents has a name (so the per-message trace
    /// line can be counted per source), and only the four translated
    /// ones have an intent other than `Ignore`.
    #[test]
    fn every_shell_hook_code_is_named_and_only_four_are_translated() {
        let documented = [
            (HSHELL_WINDOWCREATED, "HSHELL_WINDOWCREATED", Intent::Added),
            (
                HSHELL_WINDOWDESTROYED,
                "HSHELL_WINDOWDESTROYED",
                Intent::Ignore,
            ),
            (
                HSHELL_ACTIVATESHELLWINDOW,
                "HSHELL_ACTIVATESHELLWINDOW",
                Intent::Ignore,
            ),
            (
                HSHELL_WINDOWACTIVATED,
                "HSHELL_WINDOWACTIVATED",
                Intent::Foreground,
            ),
            (HSHELL_GETMINRECT, "HSHELL_GETMINRECT", Intent::Ignore),
            (HSHELL_REDRAW, "HSHELL_REDRAW", Intent::Ignore),
            (HSHELL_TASKMAN, "HSHELL_TASKMAN", Intent::Ignore),
            (HSHELL_LANGUAGE, "HSHELL_LANGUAGE", Intent::Ignore),
            (HSHELL_SYSMENU, "HSHELL_SYSMENU", Intent::Ignore),
            (HSHELL_ENDTASK, "HSHELL_ENDTASK", Intent::Ignore),
            (
                HSHELL_ACCESSIBILITYSTATE,
                "HSHELL_ACCESSIBILITYSTATE",
                Intent::Ignore,
            ),
            (HSHELL_APPCOMMAND, "HSHELL_APPCOMMAND", Intent::Ignore),
            (
                HSHELL_WINDOWREPLACED,
                "HSHELL_WINDOWREPLACED",
                Intent::Ignore,
            ),
            (
                HSHELL_WINDOWREPLACING,
                "HSHELL_WINDOWREPLACING",
                Intent::Ignore,
            ),
            (
                HSHELL_MONITORCHANGED,
                "HSHELL_MONITORCHANGED",
                Intent::Ignore,
            ),
            (
                HSHELL_RUDEAPPACTIVATED,
                "HSHELL_RUDEAPPACTIVATED",
                Intent::Foreground,
            ),
            (HSHELL_FLASH, "HSHELL_FLASH", Intent::Attention),
        ];
        // winuser.h numbers them 1..=14 and 16, plus the two high-bit forms.
        let mut codes: Vec<u32> = documented.iter().map(|&(code, _, _)| code).collect();
        codes.sort_unstable();
        let mut expected: Vec<u32> = (1..=14).collect();
        expected.extend([16, 0x8004, 0x8006]);
        assert_eq!(codes, expected);
        for (code, name, intent) in documented {
            assert_eq!(shell_hook_name(code), name, "{code:#x}");
            assert_eq!(shell_hook_intent(code), intent, "{name}");
        }
        for stray in [0, 15, 17, 0x8000, 0x8001, 0x8005, u32::MAX] {
            assert_eq!(shell_hook_name(stray), "an unknown shell hook code");
            assert_eq!(shell_hook_intent(stray), Intent::Ignore);
        }
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

    #[test]
    fn two_cloak_events_within_100ms_arm_one_snapshot_until_reset() {
        let mut d = DesktopSwitchDetector::default();
        assert!(!d.is_armed());
        // Win+Ctrl+Right: the shell cloaks the windows of one desktop and
        // uncloaks those of the other, a few ms apart.
        assert!(!d.observe(1_000));
        assert!(d.observe(1_003));
        assert!(d.is_armed());
        // The rest of the burst, and anything until the snapshot, changes nothing.
        assert!(!d.observe(1_005));
        assert!(!d.observe(1_200));
        assert!(!d.observe(1_399));
        assert!(d.is_armed());
        // The snapshot at ~1 400 ms resets; the next switch arms again.
        d.reset();
        assert!(!d.is_armed());
        assert!(!d.observe(5_000));
        assert!(d.observe(5_010));
        assert_eq!(DesktopSwitchDetector::SNAPSHOT_DELAY_MS, 400);
    }

    #[test]
    fn a_lone_cloak_event_arms_nothing_and_the_window_is_measured_from_the_first() {
        let mut d = DesktopSwitchDetector::default();
        // A window moved to another desktop, a Store app suspending, the
        // Start menu opening and closing: one event each, seconds apart.
        for now in [0, 2_000, 9_000, 20_000] {
            assert!(!d.observe(now), "{now}");
            assert!(!d.is_armed());
        }
        // Exactly the window counts; one more ms starts a new burst.
        let mut d = DesktopSwitchDetector::default();
        assert!(!d.observe(100));
        assert!(d.observe(200));
        let mut d = DesktopSwitchDetector::default();
        assert!(!d.observe(100));
        assert!(!d.observe(201));
        // ...which the next event completes.
        assert!(d.observe(250));
        // Three events spread 60 ms apart: the first two are more than the
        // window apart from the third only if measured from the first.
        let mut d = DesktopSwitchDetector::default();
        assert!(!d.observe(0));
        assert!(d.observe(60));
    }

    #[test]
    fn desktop_query_failures_are_logged_once_per_window_and_bounded() {
        let mut failures = DesktopQueryFailures::new();
        assert!(failures.is_empty());
        assert!(failures.note(0x1_0203));
        assert!(!failures.note(0x1_0203));
        assert!(failures.note(0x1_0204));
        assert_eq!(failures.len(), 2);
        // A later success forgets the refusal: the handle, reused by another
        // window that refuses, is logged again.
        failures.clear(0x1_0203);
        assert!(failures.note(0x1_0203));
        failures.clear(99);
        assert_eq!(failures.len(), 2);
        // Full, the set starts over rather than growing.
        let mut failures = DesktopQueryFailures::new();
        for id in 0..DesktopQueryFailures::CAPACITY as u64 {
            assert!(failures.note(id), "{id}");
        }
        assert!(!failures.note(0));
        assert!(failures.note(DesktopQueryFailures::CAPACITY as u64));
        assert_eq!(failures.len(), 1);
        assert!(failures.note(0));
    }
}
