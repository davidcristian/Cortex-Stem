//! Memory of processes that refused `OpenProcess`.
//!
//! `windows::enumerate` opens a window's process once per query
//! (`PROCESS_QUERY_LIMITED_INFORMATION`, which an elevated process grants
//! and a protected one refuses) and a window is queried again on every
//! event about it: a title change, a minimise, each 150 ms flush while it is
//! dragged. The open is attempted every time, because it is one cheap call
//! that fails at once and because a process id is reused once its process
//! exits, but the refusal is worth one log line per process, not one per
//! event. [`DeniedProcesses`] remembers the ids whose refusal was logged:
//! [`note`](DeniedProcesses::note) says whether an id is new, a successful
//! open [`clears`](DeniedProcesses::clear) it (the id now names a process
//! that can be opened), and the set is bounded by [`CAPACITY`]: full, it
//! starts over, so at worst a line repeats once per `CAPACITY` further
//! refusals. Plain data, unit-tested on every host.

use std::collections::BTreeSet;

/// Most process ids remembered at once.
pub const CAPACITY: usize = 256;

/// The process ids whose refusal has been logged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeniedProcesses {
    logged: BTreeSet<u32>,
}

impl DeniedProcesses {
    pub const fn new() -> Self {
        Self {
            logged: BTreeSet::new(),
        }
    }

    /// `process_id` refused the open. `true` when the refusal is new (log
    /// it); `false` when it was logged before.
    pub fn note(&mut self, process_id: u32) -> bool {
        if self.logged.contains(&process_id) {
            return false;
        }
        if self.logged.len() >= CAPACITY {
            self.logged.clear();
        }
        self.logged.insert(process_id)
    }

    /// `process_id` was opened: forget any refusal, so a refusal by the
    /// process that gets this id next is logged again.
    pub fn clear(&mut self, process_id: u32) {
        self.logged.remove(&process_id);
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
    fn a_refusal_is_new_once_per_process() {
        let mut denied = DeniedProcesses::new();
        assert!(denied.is_empty());
        assert!(denied.note(4242));
        assert!(!denied.note(4242));
        assert!(!denied.note(4242));
        // Another process is logged on its own.
        assert!(denied.note(4243));
        assert_eq!(denied.len(), 2);
    }

    #[test]
    fn a_successful_open_forgets_the_refusal() {
        let mut denied = DeniedProcesses::new();
        assert!(denied.note(4242));
        denied.clear(4242);
        assert!(denied.is_empty());
        // The id was reused by a process that refuses again: logged again.
        assert!(denied.note(4242));
        // Clearing an unknown id is a no-op.
        denied.clear(1);
        assert_eq!(denied.len(), 1);
    }

    #[test]
    fn the_set_is_bounded_and_starts_over_when_full() {
        let mut denied = DeniedProcesses::new();
        for pid in 0..CAPACITY as u32 {
            assert!(denied.note(pid), "{pid}");
        }
        assert_eq!(denied.len(), CAPACITY);
        assert!(!denied.note(0));
        // One more distinct id resets the set rather than growing it...
        assert!(denied.note(CAPACITY as u32));
        assert_eq!(denied.len(), 1);
        // ...so an old id counts as new again (one repeated line at worst).
        assert!(denied.note(0));
        assert_eq!(denied.len(), 2);
    }
}
