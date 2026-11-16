//! Virtual desktops: a desktop switch detected from the cloak events it
//! causes, and the windows whose desktop query refusal was logged.

use std::collections::BTreeSet;

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
///
/// [`TitleDebouncer`]: super::TitleDebouncer
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
