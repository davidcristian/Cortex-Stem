//! Bursts of title changes, coalesced per window.

use std::collections::BTreeMap;

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
