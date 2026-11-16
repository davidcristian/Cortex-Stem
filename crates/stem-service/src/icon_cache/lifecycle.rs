//! Opening the cache, waiting for its writer, and the bounded shutdown.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::{RecvTimeoutError, SendTimeoutError, TrySendError};
use parking_lot::Mutex;

use super::stats::Counters;
use super::writer::writer_loop;
use super::{
    CacheLimits, CacheStats, DiskIconCache, FLUSH_TIMEOUT, Job, QUEUE_CAPACITY, SHUTDOWN_DRAIN,
    SHUTDOWN_WAIT, build_id,
};

impl DiskIconCache {
    /// Open (creating it if needed) the cache in `dir` with the default
    /// bounds and start its writer. Fails when the directory cannot be
    /// created or the running build cannot be identified ([`build_id`]).
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Self> {
        Self::open_with_limits(dir, CacheLimits::default())
    }

    pub fn open_with_limits(dir: impl Into<PathBuf>, limits: CacheLimits) -> io::Result<Self> {
        Self::open_as(dir.into(), limits, build_id()?)
    }

    /// [`DiskIconCache::open_with_limits`] for the build `producer`.
    pub(super) fn open_as(dir: PathBuf, limits: CacheLimits, producer: String) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let (tx, rx) = crossbeam_channel::bounded(QUEUE_CAPACITY);
        let (stopped_tx, writer_stopped) = crossbeam_channel::bounded::<()>(0);
        let counters = Arc::new(Counters::default());
        let drain_until = Arc::new(Mutex::new(None));
        let writer = {
            let dir = dir.clone();
            let producer = producer.clone();
            let counters = Arc::clone(&counters);
            let drain_until = Arc::clone(&drain_until);
            std::thread::Builder::new()
                .name("stem-icon-cache".into())
                .spawn(move || {
                    // Dropped when this thread ends, however it ends.
                    let _stopped = stopped_tx;
                    writer_loop(&dir, limits, &producer, &rx, &counters, &drain_until);
                })?
        };
        Ok(Self {
            dir,
            producer,
            tx,
            writer: Mutex::new(Some(writer)),
            writer_stopped,
            drain_until,
            counters,
            seen: Mutex::new(HashMap::new()),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn stats(&self) -> CacheStats {
        self.counters.snapshot()
    }

    /// Hand `job` to the writer without waiting: a full queue drops it.
    pub(super) fn queue(&self, job: Job) {
        match self.tx.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => Counters::bump(&self.counters.dropped),
            // The writer is gone (shutdown): nothing to persist any more.
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    /// Wait (at most [`FLUSH_TIMEOUT`] in all) until the writer has done
    /// every job queued before this call.
    pub fn flush(&self) {
        let deadline = Instant::now() + FLUSH_TIMEOUT;
        let (done_tx, done_rx) = crossbeam_channel::bounded(1);
        if self.tx.send_deadline(Job::Flush(done_tx), deadline).is_ok() {
            let _ = done_rx.recv_deadline(deadline);
        }
    }

    /// Stop the writer and log the session's counters, blocking for at
    /// most [`SHUTDOWN_WAIT`] (it runs on the app's main thread at quit).
    /// The writer works through the jobs queued before this call for at
    /// most [`SHUTDOWN_DRAIN`] and drops the rest (counted as `abandoned`;
    /// each costs a miss at the next start). A writer that has not stopped
    /// by [`SHUTDOWN_WAIT`] (a file operation held up, by a file system
    /// filter say) is left to finish that operation on its own, drops what
    /// is left and ends; a warning says so. Reads keep working afterwards;
    /// nothing more is written. Idempotent; called on drop as well.
    pub fn shutdown(&self) {
        let Some(writer) = self.writer.lock().take() else {
            return;
        };
        let started = Instant::now();
        *self.drain_until.lock() = Some(started + SHUTDOWN_DRAIN);
        let wait_until = started + SHUTDOWN_WAIT;
        // A full queue makes the send wait for the writer to take a job;
        // a writer that is gone (it panicked) has stopped.
        let sent = !matches!(
            self.tx.send_deadline(Job::Shutdown, wait_until),
            Err(SendTimeoutError::Timeout(_))
        );
        // Nothing is ever sent on this channel: it disconnects when the
        // writer's thread ends.
        let stopped = sent
            && !matches!(
                self.writer_stopped.recv_deadline(wait_until),
                Err(RecvTimeoutError::Timeout)
            );
        if stopped {
            if writer.join().is_err() {
                log::error!("stem-icon-cache thread panicked");
            }
        } else {
            // Whatever it still holds is dropped as soon as it is free.
            *self.drain_until.lock() = Some(Instant::now());
            log::warn!(
                "icon cache: the writer did not stop within {} ms; leaving it to finish on its own (what it has queued is not written)",
                SHUTDOWN_WAIT.as_millis()
            );
            drop(writer);
        }
        let s = self.stats();
        log::info!(
            "icon cache: {} hits, {} repeats, {} misses, {} bypassed, {} written ({} failed, {} dropped, {} abandoned at quit, {} µs writing), {} evicted; {} entries, {} bytes in {}",
            s.hits,
            s.repeats,
            s.misses,
            s.bypassed,
            s.writes,
            s.write_failures,
            s.dropped,
            s.abandoned,
            s.write_nanos / 1000,
            s.evictions,
            s.entries,
            s.bytes,
            self.dir.display()
        );
    }
}

impl Drop for DiskIconCache {
    fn drop(&mut self) {
        self.shutdown();
    }
}
