//! The cache's counters: [`CacheStats`] for callers, and the atomic
//! `Counters` that the reads and the writer increment.

use std::sync::atomic::{AtomicU64, Ordering};

/// Counters since the cache was opened, for the shutdown log line and for
/// measurements. `entries` and `bytes` are what the writer last indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    /// Requests answered from disk.
    pub hits: u64,
    /// Later requests for a cacheable (key, size) this session already
    /// read from disk or loaded: answered without touching a file (from
    /// the bytes of the hit, or by the platform's memory cache).
    pub repeats: u64,
    /// Requests for a cacheable key that went to the platform.
    pub misses: u64,
    /// Requests for keys that are never cached, or whose source file could
    /// not be stamped (missing, unreadable): straight to the platform.
    pub bypassed: u64,
    /// Entries written.
    pub writes: u64,
    /// Writes that failed (logged at `debug`).
    pub write_failures: u64,
    /// Jobs dropped because the writer's queue was full.
    pub dropped: u64,
    /// Writes still queued when shutdown's drain budget
    /// ([`SHUTDOWN_DRAIN`]) ran out, dropped instead of written.
    ///
    /// [`SHUTDOWN_DRAIN`]: super::SHUTDOWN_DRAIN
    pub abandoned: u64,
    /// Entries deleted to stay within the bounds.
    pub evictions: u64,
    /// Time the writer spent encoding and writing entries, in total.
    pub write_nanos: u64,
    pub entries: u64,
    pub bytes: u64,
}

#[derive(Debug, Default)]
pub(super) struct Counters {
    pub(super) hits: AtomicU64,
    pub(super) repeats: AtomicU64,
    pub(super) misses: AtomicU64,
    pub(super) bypassed: AtomicU64,
    pub(super) writes: AtomicU64,
    pub(super) write_failures: AtomicU64,
    pub(super) dropped: AtomicU64,
    pub(super) abandoned: AtomicU64,
    pub(super) evictions: AtomicU64,
    pub(super) write_nanos: AtomicU64,
    pub(super) entries: AtomicU64,
    pub(super) bytes: AtomicU64,
}

impl Counters {
    pub(super) fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn snapshot(&self) -> CacheStats {
        let get = |c: &AtomicU64| c.load(Ordering::Relaxed);
        CacheStats {
            hits: get(&self.hits),
            repeats: get(&self.repeats),
            misses: get(&self.misses),
            bypassed: get(&self.bypassed),
            writes: get(&self.writes),
            write_failures: get(&self.write_failures),
            dropped: get(&self.dropped),
            abandoned: get(&self.abandoned),
            evictions: get(&self.evictions),
            write_nanos: get(&self.write_nanos),
            entries: get(&self.entries),
            bytes: get(&self.bytes),
        }
    }
}
