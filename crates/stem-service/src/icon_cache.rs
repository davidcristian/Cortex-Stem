//! Disk cache for icon bitmaps, behind [`crate::DockService::icon_png`].
//!
//! The platform turns an [`IconKey`] into PNG bytes and keeps what it made
//! in memory for the session (`stem_platform::windows::icons::IconCache`).
//! This cache keeps the bytes across sessions, so a warm start reads the
//! dock's icons from disk instead of extracting every one again.
//!
//! **What is cached.** Only keys whose bitmap is a pure function of one file
//! on disk ([`source_file`]): `exe:<path>` and `file:<path>` where `path` is
//! absolute, not a network path, and names a PE image (`.exe`, `.dll`,
//! `.scr`; `stem_platform::rules::icons::is_pe_image_path`), whose icon the
//! platform reads from the file's own resources. Everything else goes
//! straight to the platform, every time, exactly as without this cache:
//! - `hwnd:` keys (a live window's icon, which the window may change);
//! - `shell:` keys (the Recycle Bin's full and empty states change on their
//!   own; the UI draws them anyway);
//! - `aumid:` keys (a packaged app's logo: the key carries no version and
//!   the service cannot see the package's, so an updated package could not
//!   be told from the old one);
//! - `file:` keys of anything but a PE image (a shortcut's icon follows its
//!   target, a document's its file association), relative paths, and any
//!   namespace this module does not know (the mock's `mock:`);
//! - paths that start with two slashes or backslashes (`\\server\share\…`,
//!   `\\?\UNC\…`, and `\\?\` verbatim paths with them): stamping a file on
//!   a share that went offline blocks for the redirector's timeout, which
//!   the request would then pay again in the platform. A share mapped to a
//!   drive letter cannot be told from a local disk here and is cached.
//!
//! **Validity.** Each entry stores the source file's length and modification
//! time ([`Stamp`]), taken before the platform loaded the bitmap and checked
//! again after; a read compares them with the file as it is now, so a
//! replaced or updated exe misses. A packaged app's exe also has the
//! package version in its path (`WindowsApps\<name>_<version>_…`), so an
//! updated package is a new key. Entries also store the build of Stem that
//! wrote them ([`build_id`]: the crate version and the running executable's
//! length and modification time), and a read accepts only the running
//! build's. The version number alone would not do: it has stayed `0.1.0`
//! while the icon pipeline in `stem-platform` changed,
//! and the service cannot see that crate's changes. So every new `stem.exe`
//! (a rebuild, an install, an update) starts cold once and never meets an
//! older pipeline's bitmaps. A development build and the installed one share
//! the folder (both are `dev.davidcristian.stem`) and replace each other's
//! entries, so each switch between them costs one cold start. When the
//! running executable cannot be stamped, [`DiskIconCache::open`] fails and
//! the service runs without the cache.
//!
//! **Within a session.** Each (key, size) goes to the disk once per session
//! at most. The first request stamps the source file and reads the entry;
//! on a miss it asks the platform and queues the answer for writing. Later
//! requests touch no file: an entry read from disk is answered from the
//! bytes this cache kept, and anything the platform loaded is asked of the
//! platform again, whose memory cache holds it (so the memory held is what
//! the platform's cache alone would hold). Like that memory cache, neither
//! notices a file that changes while Stem runs. The disk keeps the entry
//! with the old stamp, which misses at the next start, where a fresh load is
//! written. Only a session's first platform load of a (key, size) is
//! written, and only when the file did not change during it
//! ([`DiskIconCache::admit`]): a second, racing load may get the
//! platform's memory copy of an older file.
//!
//! **Layout.** One file per (key, requested size) in the cache directory,
//! named `<16 hex digits>.icon` (FNV-1a 64 of the key and the size; the
//! entry repeats both, so a collision is a miss, never a wrong icon). An
//! entry is, little-endian: the magic `STEMICON`, the format version (u32),
//! the build id, the key (each a u32 length and bytes), the
//! requested size (u32), the source length and modification time (u64
//! each, the time in nanoseconds since the Unix epoch), the PNG (u32 length
//! and bytes) and a 64-bit checksum of everything before it. Writes are
//! atomic: a temp file `<hex>.<pid>-<n>.tmp` is written and renamed over the
//! entry. Nothing is `fsync`ed; a torn entry after a power cut fails its
//! checksum.
//!
//! **Bounds.** At most [`DEFAULT_MAX_FILES`] entries and [`DEFAULT_MAX_BYTES`]
//! bytes ([`CacheLimits`]); past either, the least recently used entries are
//! deleted after each write. "Used" is the last write or disk hit (the
//! session's later requests, answered from memory, do not count); a hit
//! refreshes the file's modification time when it is older than
//! [`TOUCH_PERSIST_AFTER`], so recency survives a restart at that
//! granularity. A PNG larger than [`MAX_PAYLOAD_BYTES`] or a key longer
//! than [`MAX_KEY_BYTES`] is not cached, and a read never loads more than
//! one entry's maximum size.
//!
//! **Threads.** Reads happen on the caller's thread (a hit is a `metadata`
//! of the source file plus one small file read; a later request in the
//! session is a map lookup). Writes, hits' recency
//! updates and deletions are queued for one writer thread
//! (`stem-icon-cache`) on a bounded queue ([`QUEUE_CAPACITY`]); when the
//! queue is full the job is dropped, never waited for, so a miss costs its
//! caller one extra `metadata` and a copy of the PNG. The writer scans the
//! directory when it starts (indexing entries, deleting stray temp files,
//! evicting down to the bounds). [`DiskIconCache::shutdown`] runs on the
//! app's main thread at quit and is bounded: the writer works through its
//! queue for at most [`SHUTDOWN_DRAIN`] and drops the rest (a later miss),
//! and shutdown waits for it at most [`SHUTDOWN_WAIT`]; a writer stuck in a
//! file operation is left to finish it on its own, with a warning.
//!
//! **Logging.** One `debug` line per disk hit (`icon <key>: <size> px from
//! the disk cache`, the counterpart of the platform's one line per real
//! load; none for the session's later requests)
//! and one `info` line at shutdown with the session's [`CacheStats`].
//!
//! **Failure.** Nothing here is ever an error to the caller: a missing,
//! unreadable, corrupt, stale or foreign entry is a miss (a corrupt one is
//! deleted), and a failed write is logged at `debug` and counted. The
//! platform's own errors pass through unchanged. Without a cache directory
//! (the service was started without one, or it could not be created) the
//! service calls the platform directly.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, RecvTimeoutError, SendTimeoutError, Sender, TrySendError};
use parking_lot::Mutex;
use stem_core::IconKey;
use stem_platform::PlatformResult;
use stem_platform::rules::icons::is_pe_image_path;

/// Default bound on the total size of the entries (64 MiB). A dock with 40
/// apps at three sizes holds about 120 entries of 5–60 KB: a few MiB.
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Default bound on the number of entries.
pub const DEFAULT_MAX_FILES: usize = 4096;
/// Largest PNG that is cached (1 MiB; a 256 px RGBA icon is at most about
/// 270 KB even uncompressed).
pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
/// Longest key that is cached, in bytes.
pub const MAX_KEY_BYTES: usize = 4096;
/// Jobs (writes, recency updates, deletions) the writer may have queued;
/// past that, new jobs are dropped.
pub const QUEUE_CAPACITY: usize = 256;
/// A hit refreshes its file's modification time (the recency a restart
/// sees) only when that time is at least this old.
pub const TOUCH_PERSIST_AFTER: Duration = Duration::from_secs(60 * 60);
/// How long [`DiskIconCache::shutdown`] lets the writer work through the
/// jobs queued before it; it drops the rest (each a miss at the next
/// start). About 100–400 writes at the 1–4 ms one takes.
pub const SHUTDOWN_DRAIN: Duration = Duration::from_millis(400);
/// The longest [`DiskIconCache::shutdown`] blocks its caller (the app's
/// main thread at quit) waiting for the writer to stop.
pub const SHUTDOWN_WAIT: Duration = Duration::from_millis(500);

const MAGIC: &[u8; 8] = b"STEMICON";
const FORMAT_VERSION: u32 = 1;
const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
const ENTRY_EXTENSION: &str = ".icon";
const TEMP_EXTENSION: &str = ".tmp";
const NAME_DIGITS: usize = 16;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// The largest entry a read accepts: the payload bound plus room for the
/// header, the key and the build id.
const MAX_ENTRY_BYTES: usize = MAX_PAYLOAD_BYTES + MAX_KEY_BYTES + 256;
/// How long [`DiskIconCache::flush`] waits for the writer at most.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(10);

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// How big the cache may grow; see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheLimits {
    pub max_bytes: u64,
    pub max_files: usize,
}

impl Default for CacheLimits {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            max_files: DEFAULT_MAX_FILES,
        }
    }
}

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
    pub abandoned: u64,
    /// Entries deleted to stay within the bounds.
    pub evictions: u64,
    /// Time the writer spent encoding and writing entries, in total.
    pub write_nanos: u64,
    pub entries: u64,
    pub bytes: u64,
}

#[derive(Debug, Default)]
struct Counters {
    hits: AtomicU64,
    repeats: AtomicU64,
    misses: AtomicU64,
    bypassed: AtomicU64,
    writes: AtomicU64,
    write_failures: AtomicU64,
    dropped: AtomicU64,
    abandoned: AtomicU64,
    evictions: AtomicU64,
    write_nanos: AtomicU64,
    entries: AtomicU64,
    bytes: AtomicU64,
}

impl Counters {
    fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> CacheStats {
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

/// What a cacheable bitmap depends on: its source file's length and
/// modification time (nanoseconds since the Unix epoch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub len: u64,
    pub modified_ns: u64,
}

impl Stamp {
    /// The file's stamp now, or `None` when it is not a readable regular
    /// file or its time cannot be expressed.
    pub fn of(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let since_epoch = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some(Self {
            len: meta.len(),
            modified_ns: u64::try_from(since_epoch.as_nanos()).ok()?,
        })
    }
}

/// What identifies the build of Stem that writes and reads entries: the
/// crate version and the length and modification time of the running
/// executable, `<version>+<length>.<nanoseconds>`. Every rebuild or install
/// of `stem.exe` changes it, including a change to the icon pipeline in
/// another crate that the version number does not show. An error when the
/// executable cannot be found or stamped.
pub fn build_id() -> io::Result<String> {
    let exe = std::env::current_exe()?;
    let stamp = Stamp::of(&exe).ok_or_else(|| {
        io::Error::other(format!(
            "the running executable {} cannot be stamped",
            exe.display()
        ))
    })?;
    Ok(format!(
        "{CRATE_VERSION}+{}.{}",
        stamp.len, stamp.modified_ns
    ))
}

/// The file whose contents alone decide `key`'s bitmap, if there is one;
/// `None` for every key that must not be cached (see the module docs).
pub fn source_file(key: &IconKey) -> Option<&Path> {
    let (namespace, value) = key.0.split_once(':')?;
    if !matches!(namespace, "exe" | "file") || key.0.len() > MAX_KEY_BYTES {
        return None;
    }
    let path = Path::new(value);
    (is_pe_image_path(value) && path.is_absolute() && !is_network_path(value)).then_some(path)
}

/// `\\server\share\…`, `\\?\UNC\…` and anything else that starts with two
/// slashes or backslashes (`\\?\C:\…` too: rare in keys, and merely not
/// cached).
fn is_network_path(value: &str) -> bool {
    let separator = |b: u8| b == b'\\' || b == b'/';
    matches!(value.as_bytes(), [a, b, ..] if separator(*a) && separator(*b))
}

enum Job {
    Write {
        name: String,
        key: IconKey,
        size: u32,
        stamp: Stamp,
        png: Vec<u8>,
    },
    Touch {
        name: String,
    },
    Remove {
        name: String,
    },
    Flush(Sender<()>),
    Shutdown,
    /// Hold the writer up for a while, as a stuck file operation would.
    #[cfg(test)]
    Stall(Duration),
}

/// What this session already did with a cacheable (key, size).
#[derive(Debug)]
enum Seen {
    /// Read from disk: later requests get these bytes.
    Hit(Vec<u8>),
    /// Loaded by the platform, whose memory cache now holds the bitmap
    /// (written or not). Later requests go to the platform.
    Loaded,
}

/// What a read found.
enum Lookup {
    Hit(Vec<u8>),
    /// No usable entry: none, unreadable, another key's, or stale.
    Miss,
    /// An entry that is not one (truncated, checksum, foreign bytes): a miss
    /// that also deletes it.
    Corrupt,
}

/// The disk icon cache; see the module docs. Dropping it shuts the writer
/// down as [`DiskIconCache::shutdown`] does.
#[derive(Debug)]
pub struct DiskIconCache {
    dir: PathBuf,
    /// The build id ([`build_id`]) this cache writes and accepts.
    producer: String,
    tx: Sender<Job>,
    writer: Mutex<Option<JoinHandle<()>>>,
    /// Disconnects when the writer's thread ends (it holds the sender).
    writer_stopped: Receiver<()>,
    /// Set by `shutdown`: from then on the writer drops the jobs it takes
    /// instead of doing them.
    drain_until: Arc<Mutex<Option<Instant>>>,
    counters: Arc<Counters>,
    /// What this session did with each cacheable (key, size) it answered;
    /// see the module docs, "Within a session".
    seen: Mutex<HashMap<(IconKey, u32), Seen>>,
}

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
    fn open_as(dir: PathBuf, limits: CacheLimits, producer: String) -> io::Result<Self> {
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

    /// `key`'s bitmap at `size`: from disk when a valid entry exists, else
    /// from `load` (the platform), whose `Some` result is queued for
    /// writing when the key is cacheable and the source file did not change
    /// meanwhile. A (key, size) this session already answered touches no
    /// file (module docs, "Within a session"). `load`'s errors are returned
    /// unchanged; the cache itself never fails.
    pub fn get_or_load(
        &self,
        key: &IconKey,
        size: u32,
        load: impl FnOnce() -> PlatformResult<Option<Vec<u8>>>,
    ) -> PlatformResult<Option<Vec<u8>>> {
        let Some(source) = source_file(key) else {
            Counters::bump(&self.counters.bypassed);
            return load();
        };
        let slot = (key.clone(), size);
        // `Some(Some(png))`: a hit this session; `Some(None)`: loaded by
        // the platform this session. The lock is not held while loading.
        let seen = self.seen.lock().get(&slot).map(|seen| match seen {
            Seen::Hit(png) => Some(png.clone()),
            Seen::Loaded => None,
        });
        match seen {
            Some(Some(png)) => {
                Counters::bump(&self.counters.repeats);
                return Ok(Some(png));
            }
            Some(None) => {
                Counters::bump(&self.counters.repeats);
                return load();
            }
            None => {}
        }

        let name = entry_name(key, size);
        let before = Stamp::of(source);
        match before {
            Some(stamp) => match self.read(&name, key, size, stamp) {
                Lookup::Hit(png) => {
                    Counters::bump(&self.counters.hits);
                    log::debug!("icon {}: {size} px from the disk cache", key.0);
                    self.queue(Job::Touch { name });
                    self.seen
                        .lock()
                        .entry(slot)
                        .or_insert_with(|| Seen::Hit(png.clone()));
                    return Ok(Some(png));
                }
                Lookup::Miss => Counters::bump(&self.counters.misses),
                Lookup::Corrupt => {
                    Counters::bump(&self.counters.misses);
                    self.queue(Job::Remove { name: name.clone() });
                }
            },
            None => Counters::bump(&self.counters.bypassed),
        }

        let answer = load()?;
        if let Some(png) = &answer {
            let after = before.and_then(|_| Stamp::of(source));
            if let Some(stamp) = self.admit(slot, before, after)
                && png.len() <= MAX_PAYLOAD_BYTES
            {
                self.queue(Job::Write {
                    name,
                    key: key.clone(),
                    size,
                    stamp,
                    png: png.clone(),
                });
            }
        }
        Ok(answer)
    }

    /// Record that the platform loaded (key, size) between the stamps
    /// `before` and `after`, and say whether its answer may be written, and
    /// under which stamp. Only this session's first load of (key, size) is
    /// written, and only when the file did not change during it: the
    /// platform keeps that first bitmap for the session, so a second load
    /// (one that raced the first past the session check) may be its memory
    /// copy of a file that has changed since.
    fn admit(
        &self,
        slot: (IconKey, u32),
        before: Option<Stamp>,
        after: Option<Stamp>,
    ) -> Option<Stamp> {
        let current = before.filter(|b| after == Some(*b));
        match self.seen.lock().entry(slot) {
            Entry::Vacant(vacant) => {
                if before.is_some() && current.is_none() {
                    log::debug!(
                        "icon {} ({} px) changed on disk while it loaded; not cached until the next start",
                        vacant.key().0.0,
                        vacant.key().1
                    );
                }
                vacant.insert(Seen::Loaded);
                current
            }
            Entry::Occupied(_) => None,
        }
    }

    fn read(&self, name: &str, key: &IconKey, size: u32, stamp: Stamp) -> Lookup {
        let path = self.dir.join(name);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::debug!("icon cache entry {} unreadable: {e}", path.display());
                }
                return Lookup::Miss;
            }
        };
        let mut bytes = Vec::new();
        // `MAX_ENTRY_BYTES` fits in u64 on every target.
        let limit = u64::try_from(MAX_ENTRY_BYTES).unwrap_or(u64::MAX);
        if let Err(e) = file.take(limit.saturating_add(1)).read_to_end(&mut bytes) {
            log::debug!("icon cache entry {} unreadable: {e}", path.display());
            return Lookup::Miss;
        }
        if bytes.len() > MAX_ENTRY_BYTES {
            return Lookup::Corrupt;
        }
        let Some(entry) = decode(&bytes) else {
            log::debug!(
                "icon cache entry {} is corrupt; deleting it",
                path.display()
            );
            return Lookup::Corrupt;
        };
        if entry.producer != self.producer.as_bytes()
            || entry.key != key.0.as_bytes()
            || entry.size != size
            || entry.stamp != stamp
        {
            return Lookup::Miss;
        }
        Lookup::Hit(entry.png.to_vec())
    }

    /// Hand `job` to the writer without waiting: a full queue drops it.
    fn queue(&self, job: Job) {
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

// ----- the writer -------------------------------------------------------------

/// An indexed entry: its size on disk, when it was last used, and the
/// modification time its file carries (the recency a restart sees).
#[derive(Debug, Clone, Copy)]
struct Slot {
    len: u64,
    used: SystemTime,
    persisted: SystemTime,
}

#[derive(Debug, Default)]
struct Index {
    slots: HashMap<String, Slot>,
    bytes: u64,
}

impl Index {
    /// Index the entries in `dir`, deleting stray temp files of earlier
    /// runs. Files that are neither are left alone and not counted.
    fn scan(dir: &Path) -> Self {
        let mut index = Self::default();
        let listing = match fs::read_dir(dir) {
            Ok(listing) => listing,
            Err(e) => {
                log::debug!("icon cache {}: cannot list: {e}", dir.display());
                return index;
            }
        };
        for item in listing.flatten() {
            let Ok(name) = item.file_name().into_string() else {
                continue;
            };
            if is_temp_name(&name) {
                if let Err(e) = fs::remove_file(item.path()) {
                    log::debug!("icon cache: stray temp file {name} not deleted: {e}");
                }
                continue;
            }
            if !is_entry_name(&name) {
                continue;
            }
            let Ok(meta) = item.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            index.insert(
                name,
                Slot {
                    len: meta.len(),
                    used: modified,
                    persisted: modified,
                },
            );
        }
        index
    }

    fn insert(&mut self, name: String, slot: Slot) {
        self.bytes += slot.len;
        if let Some(old) = self.slots.insert(name, slot) {
            self.bytes -= old.len;
        }
    }

    fn forget(&mut self, name: &str) {
        if let Some(old) = self.slots.remove(name) {
            self.bytes -= old.len;
        }
    }

    fn over(&self, limits: CacheLimits) -> bool {
        self.slots.len() > limits.max_files || self.bytes > limits.max_bytes
    }

    /// Delete least recently used entries (never `keep`) until the index
    /// is within `limits`. Returns how many were deleted.
    fn evict(&mut self, dir: &Path, limits: CacheLimits, keep: Option<&str>) -> u64 {
        if !self.over(limits) {
            return 0;
        }
        let mut order: Vec<(SystemTime, String)> = self
            .slots
            .iter()
            .filter(|(name, _)| Some(name.as_str()) != keep)
            .map(|(name, slot)| (slot.used, name.clone()))
            .collect();
        order.sort();
        let mut evicted = 0;
        for (_, name) in order {
            if !self.over(limits) {
                break;
            }
            match fs::remove_file(dir.join(&name)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                // Forgotten anyway, so a file that cannot be deleted does
                // not stall eviction; the next start counts it again.
                Err(e) => log::debug!("icon cache: {name} not evicted: {e}"),
            }
            self.forget(&name);
            evicted += 1;
        }
        evicted
    }

    /// Mark `name` used now; persist that in its modification time when
    /// the time it carries is older than [`TOUCH_PERSIST_AFTER`].
    fn touch(&mut self, dir: &Path, name: &str) {
        let Some(slot) = self.slots.get_mut(name) else {
            return;
        };
        let now = SystemTime::now();
        slot.used = now;
        let stale = now
            .duration_since(slot.persisted)
            .is_ok_and(|age| age >= TOUCH_PERSIST_AFTER);
        if !stale {
            return;
        }
        let touched = OpenOptions::new()
            .write(true)
            .open(dir.join(name))
            .and_then(|file| file.set_modified(now));
        match touched {
            Ok(()) => slot.persisted = now,
            Err(e) => log::debug!("icon cache: recency of {name} not saved: {e}"),
        }
    }
}

fn writer_loop(
    dir: &Path,
    limits: CacheLimits,
    producer: &str,
    rx: &Receiver<Job>,
    counters: &Counters,
    drain_until: &Mutex<Option<Instant>>,
) {
    let mut index = Index::scan(dir);
    let evicted = index.evict(dir, limits, None);
    counters.evictions.fetch_add(evicted, Ordering::Relaxed);
    publish(&index, counters);
    let mut temp_seq: u64 = 0;

    for job in rx {
        // Past shutdown's drain budget, file work is dropped, not done.
        let out_of_time = drain_until
            .lock()
            .is_some_and(|until| Instant::now() >= until);
        match job {
            Job::Write { .. } if out_of_time => Counters::bump(&counters.abandoned),
            Job::Touch { .. } | Job::Remove { .. } if out_of_time => {}
            Job::Write {
                name,
                key,
                size,
                stamp,
                png,
            } => {
                let started = Instant::now();
                let bytes = encode(producer, &key.0, size, stamp, &png);
                temp_seq += 1;
                let temp = temp_name(&name, temp_seq);
                match write_atomic(dir, &name, &temp, &bytes) {
                    Ok(()) => {
                        Counters::bump(&counters.writes);
                        let now = SystemTime::now();
                        index.insert(
                            name.clone(),
                            Slot {
                                len: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                                used: now,
                                persisted: now,
                            },
                        );
                        let evicted = index.evict(dir, limits, Some(&name));
                        counters.evictions.fetch_add(evicted, Ordering::Relaxed);
                    }
                    Err(e) => {
                        Counters::bump(&counters.write_failures);
                        log::debug!("icon cache: {} ({size} px) not written: {e}", key.0);
                    }
                }
                let took = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                counters.write_nanos.fetch_add(took, Ordering::Relaxed);
            }
            Job::Touch { name } => index.touch(dir, &name),
            Job::Remove { name } => {
                match fs::remove_file(dir.join(&name)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => log::debug!("icon cache: corrupt {name} not deleted: {e}"),
                }
                index.forget(&name);
            }
            Job::Flush(done) => {
                let _ = done.send(());
            }
            Job::Shutdown => break,
            #[cfg(test)]
            Job::Stall(pause) => std::thread::sleep(pause),
        }
        publish(&index, counters);
    }
    log::debug!("stem-icon-cache writer stopped");
}

fn publish(index: &Index, counters: &Counters) {
    let entries = u64::try_from(index.slots.len()).unwrap_or(u64::MAX);
    counters.entries.store(entries, Ordering::Relaxed);
    counters.bytes.store(index.bytes, Ordering::Relaxed);
}

/// Write `bytes` to `dir/temp`, then rename it over `dir/name`. The
/// directory is recreated once if it went away while Stem ran.
fn write_atomic(dir: &Path, name: &str, temp: &str, bytes: &[u8]) -> io::Result<()> {
    match write_then_rename(dir, name, temp, bytes) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(dir)?;
            write_then_rename(dir, name, temp, bytes)
        }
        other => other,
    }
}

fn write_then_rename(dir: &Path, name: &str, temp: &str, bytes: &[u8]) -> io::Result<()> {
    let temp_path = dir.join(temp);
    let result = File::create(&temp_path)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| fs::rename(&temp_path, dir.join(name)));
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

// ----- names and the entry format ---------------------------------------------

/// `<FNV-1a 64 of the key, a NUL and the size>.icon`.
fn entry_name(key: &IconKey, size: u32) -> String {
    let hash = key
        .0
        .bytes()
        .chain([0])
        .chain(size.to_le_bytes())
        .fold(FNV_OFFSET, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(FNV_PRIME)
        });
    format!("{hash:016x}{ENTRY_EXTENSION}")
}

fn temp_name(entry: &str, seq: u64) -> String {
    let stem = entry.strip_suffix(ENTRY_EXTENSION).unwrap_or(entry);
    format!("{stem}.{}-{seq}{TEMP_EXTENSION}", std::process::id())
}

fn is_hex_stem(name: &str) -> bool {
    name.len() >= NAME_DIGITS
        && name.as_bytes()[..NAME_DIGITS]
            .iter()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_entry_name(name: &str) -> bool {
    name.len() == NAME_DIGITS + ENTRY_EXTENSION.len()
        && is_hex_stem(name)
        && name.ends_with(ENTRY_EXTENSION)
}

fn is_temp_name(name: &str) -> bool {
    is_hex_stem(name)
        && name.as_bytes().get(NAME_DIGITS) == Some(&b'.')
        && name.ends_with(TEMP_EXTENSION)
}

/// A 64-bit checksum against torn writes and bit rot (not an adversary):
/// FNV-style mixing over 8-byte words, so a hit costs little even for a
/// large PNG. Every step is a bijection of the state, so a change confined
/// to one word always changes the result.
fn checksum(bytes: &[u8]) -> u64 {
    let words = bytes.chunks_exact(8);
    let rest = words.remainder();
    let mut h = FNV_OFFSET ^ u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    for word in words {
        // Every chunk of `chunks_exact(8)` has 8 bytes.
        let word: [u8; 8] = word.try_into().unwrap_or_default();
        h = (h ^ u64::from_le_bytes(word))
            .wrapping_mul(FNV_PRIME)
            .rotate_left(29);
    }
    for &b in rest {
        h = (h ^ u64::from(b)).wrapping_mul(FNV_PRIME);
    }
    h
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    // Callers bound every field far below 4 GiB; a length that did not fit
    // would make the entry fail to decode, which is a miss.
    let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
}

fn encode(producer: &str, key: &str, size: u32, stamp: Stamp, png: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + producer.len() + key.len() + png.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    put_bytes(&mut out, producer.as_bytes());
    put_bytes(&mut out, key.as_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&stamp.len.to_le_bytes());
    out.extend_from_slice(&stamp.modified_ns.to_le_bytes());
    put_bytes(&mut out, png);
    let sum = checksum(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    out
}

#[derive(Debug, PartialEq, Eq)]
struct Decoded<'a> {
    producer: &'a [u8],
    key: &'a [u8],
    size: u32,
    stamp: Stamp,
    png: &'a [u8],
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Some(head)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, tail) = self.0.split_first_chunk::<N>()?;
        self.0 = tail;
        Some(*head)
    }

    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }

    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = usize::try_from(self.u32()?).ok()?;
        self.take(n)
    }
}

/// The entry in `bytes`, or `None` when it is not a well-formed entry of
/// this format with a matching checksum and a PNG payload.
fn decode(bytes: &[u8]) -> Option<Decoded<'_>> {
    let (body, sum) = bytes.split_last_chunk::<8>()?;
    let mut r = Reader(body);
    if r.take(MAGIC.len())? != MAGIC || r.u32()? != FORMAT_VERSION {
        return None;
    }
    if u64::from_le_bytes(*sum) != checksum(body) {
        return None;
    }
    let producer = r.bytes()?;
    let key = r.bytes()?;
    let size = r.u32()?;
    let stamp = Stamp {
        len: r.u64()?,
        modified_ns: r.u64()?,
    };
    let png = r.bytes()?;
    if !r.0.is_empty() || !png.starts_with(&PNG_SIGNATURE) {
        return None;
    }
    Some(Decoded {
        producer,
        key,
        size,
        stamp,
        png,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::AtomicUsize;

    use pretty_assertions::assert_eq;
    use stem_platform::mock::MockPlatform;
    use stem_platform::{Platform, PlatformError};

    use super::*;

    /// A fresh directory under the system temp dir, removed on drop.
    pub(crate) struct TempDir(PathBuf);

    impl TempDir {
        pub(crate) fn new(tag: &str) -> Self {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let n = SEQ.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir()
                .join(format!("stem-icon-cache-{tag}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }

        /// A fake executable with `contents`; its `exe:` key.
        pub(crate) fn exe(&self, name: &str, contents: &[u8]) -> IconKey {
            let path = self.0.join(name);
            fs::write(&path, contents).unwrap();
            IconKey(format!("exe:{}", path.display()))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// `len` bytes that pass for a PNG: the signature, then `tag`s.
    pub(crate) fn png(tag: u8, len: usize) -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        out.resize(len.max(PNG_SIGNATURE.len()), tag);
        out
    }

    /// A loader that counts its calls and answers `answer`.
    struct Loader {
        calls: AtomicUsize,
    }

    impl Loader {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }

        fn answer(
            &self,
            answer: PlatformResult<Option<Vec<u8>>>,
        ) -> PlatformResult<Option<Vec<u8>>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            answer
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.ends_with(ENTRY_EXTENSION) || n.ends_with(TEMP_EXTENSION))
            .collect();
        names.sort();
        names
    }

    fn cache_dir(root: &TempDir) -> PathBuf {
        root.path().join("icons")
    }

    #[test]
    fn source_files_are_absolute_pe_images_under_exe_and_file_only() {
        let root = TempDir::new("source");
        let exe = root.exe("app.exe", b"MZ");
        assert!(source_file(&exe).is_some());
        let dll = IconKey(format!("file:{}", root.path().join("x.DLL").display()));
        assert!(source_file(&dll).is_some());
        for uncached in [
            IconKey("hwnd:1234:56".into()),
            IconKey::shell("recycle-bin-full"),
            IconKey::shell("recycle-bin-empty"),
            IconKey("aumid:Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
            IconKey("mock:exe:x".into()),
            IconKey(format!("file:{}", root.path().join("app.lnk").display())),
            IconKey(format!("file:{}", root.path().join("notes.txt").display())),
            IconKey("exe:relative/app.exe".into()),
            IconKey(r"exe:\\server\share\app.exe".into()),
            IconKey(r"file:\\?\UNC\server\share\app.exe".into()),
            IconKey("exe://server/share/app.exe".into()),
            IconKey("exe:".into()),
            IconKey("nonsense".into()),
            IconKey(format!(
                "exe:{}{}.exe",
                root.path().display(),
                "a".repeat(MAX_KEY_BYTES)
            )),
        ] {
            assert_eq!(source_file(&uncached), None, "{uncached:?}");
        }
    }

    #[test]
    fn entries_round_trip_and_any_changed_byte_fails_to_decode() {
        let stamp = Stamp {
            len: 12345,
            modified_ns: 1_700_000_000_123_456_789,
        };
        let payload = png(7, 61);
        let build = "0.1.0+123456.1700000000000000000";
        let bytes = encode(build, "exe:C:/x.exe", 96, stamp, &payload);
        assert_eq!(
            decode(&bytes),
            Some(Decoded {
                producer: build.as_bytes(),
                key: b"exe:C:/x.exe",
                size: 96,
                stamp,
                png: &payload,
            })
        );
        for i in 0..bytes.len() {
            let mut bad = bytes.clone();
            bad[i] ^= 0x10;
            assert_eq!(decode(&bad), None, "flipped byte {i}");
        }
        for len in 0..bytes.len() {
            assert_eq!(decode(&bytes[..len]), None, "truncated to {len}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(decode(&longer), None);
        // A payload that is not a PNG is refused even with a good checksum.
        assert_eq!(decode(&encode(build, "k", 1, stamp, b"GIF89a..")), None);
    }

    #[test]
    fn entries_carry_the_running_build_and_another_build_misses() {
        let id = build_id().unwrap();
        let exe = Stamp::of(&std::env::current_exe().unwrap()).unwrap();
        assert_eq!(
            id,
            format!("{CRATE_VERSION}+{}.{}", exe.len, exe.modified_ns)
        );

        let root = TempDir::new("build");
        let key = root.exe("app.exe", b"MZ");
        let dir = cache_dir(&root);
        {
            let cache = DiskIconCache::open(&dir).unwrap();
            cache
                .get_or_load(&key, 48, || Ok(Some(png(1, 100))))
                .unwrap();
            cache.flush();
        }
        let bytes = fs::read(dir.join(entry_name(&key, 48))).unwrap();
        assert_eq!(decode(&bytes).unwrap().producer, id.as_bytes());

        // Another stem.exe of the same version (rebuilt, installed,
        // updated) misses and writes its own bitmap.
        let loader = Loader::new();
        {
            let other = DiskIconCache::open_as(
                dir.clone(),
                CacheLimits::default(),
                format!("{CRATE_VERSION}+1.2"),
            )
            .unwrap();
            let got = other
                .get_or_load(&key, 48, || loader.answer(Ok(Some(png(2, 100)))))
                .unwrap();
            assert_eq!(got, Some(png(2, 100)));
            other.flush();
        }
        assert_eq!(loader.calls(), 1);
        // And the running build misses that one in turn.
        let cache = DiskIconCache::open(&dir).unwrap();
        let got = cache
            .get_or_load(&key, 48, || loader.answer(Ok(Some(png(3, 100)))))
            .unwrap();
        assert_eq!(got, Some(png(3, 100)));
        assert_eq!(loader.calls(), 2);
    }

    #[test]
    fn names_are_stable_hex_and_differ_by_key_and_size() {
        let a = entry_name(&IconKey("exe:C:/a.exe".into()), 48);
        assert!(is_entry_name(&a), "{a}");
        assert_ne!(a, entry_name(&IconKey("exe:C:/a.exe".into()), 96));
        assert_ne!(a, entry_name(&IconKey("exe:C:/b.exe".into()), 48));
        // Fixed across builds: a restart must find what the last run wrote.
        assert_eq!(
            entry_name(&IconKey("exe:C:/a.exe".into()), 48),
            entry_name(&IconKey("exe:C:/a.exe".into()), 48)
        );
        let temp = temp_name(&a, 3);
        assert!(is_temp_name(&temp), "{temp}");
        assert!(!is_entry_name(&temp));
        assert!(!is_temp_name(&a));
        assert!(!is_entry_name("notes.txt"));
        assert!(!is_entry_name("0123456789ABCDEF.icon"));
    }

    #[test]
    fn a_cacheable_icon_is_loaded_once_and_read_from_disk_after_a_restart() {
        let root = TempDir::new("restart");
        let key = root.exe("app.exe", b"MZ version 1");
        let mock = MockPlatform::new();
        let loader = Loader::new();
        let first = {
            let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
            let got = cache
                .get_or_load(&key, 96, || loader.answer(mock.icon_png(&key, 96)))
                .unwrap()
                .unwrap();
            cache.flush();
            assert_eq!(cache.stats().misses, 1);
            assert_eq!(cache.stats().writes, 1);
            assert_eq!(cache.stats().entries, 1);
            got
        };
        assert_eq!(loader.calls(), 1);
        assert_eq!(entries(&cache_dir(&root)).len(), 1);

        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        for _ in 0..3 {
            let got = cache
                .get_or_load(&key, 96, || loader.answer(mock.icon_png(&key, 96)))
                .unwrap();
            assert_eq!(got.as_deref(), Some(first.as_slice()));
        }
        assert_eq!(loader.calls(), 1, "a hit never asks the platform");
        // Another size is another entry.
        cache
            .get_or_load(&key, 48, || loader.answer(mock.icon_png(&key, 48)))
            .unwrap();
        assert_eq!(loader.calls(), 2);
        cache.flush();
        let stats = cache.stats();
        // One disk hit; the two requests after it come from memory.
        assert_eq!(
            (stats.hits, stats.repeats, stats.misses, stats.writes),
            (1, 2, 1, 1)
        );
        assert_eq!(stats.entries, 2);
        assert_eq!(entries(&cache_dir(&root)).len(), 2, "no temp file left");
    }

    #[test]
    fn a_changed_exe_misses_whether_its_size_or_only_its_time_changed() {
        let root = TempDir::new("changed");
        let key = root.exe("app.exe", b"MZ version 1");
        let path = source_file(&key).unwrap().to_path_buf();
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        let loader = Loader::new();
        let load = |tag| loader.answer(Ok(Some(png(tag, 100))));

        assert_eq!(
            cache.get_or_load(&key, 48, || load(1)).unwrap(),
            Some(png(1, 100))
        );
        cache.flush();
        drop(cache);

        // A new build of the exe (another length) misses after a restart
        // and its icon replaces the old entry.
        fs::write(&path, b"MZ version 2, longer").unwrap();
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        assert_eq!(
            cache.get_or_load(&key, 48, || load(2)).unwrap(),
            Some(png(2, 100))
        );
        assert_eq!(loader.calls(), 2);
        cache.flush();
        drop(cache);

        // Same length, other contents and time: still a miss.
        fs::write(&path, b"MZ version 3, longer").unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        assert_eq!(
            cache.get_or_load(&key, 48, || load(3)).unwrap(),
            Some(png(3, 100))
        );
        assert_eq!(loader.calls(), 3);
        cache.flush();
        drop(cache);

        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        assert_eq!(
            cache.get_or_load(&key, 48, || load(9)).unwrap(),
            Some(png(3, 100))
        );
        assert_eq!(loader.calls(), 3);
        assert_eq!(entries(&cache_dir(&root)).len(), 1);
    }

    #[test]
    fn a_file_that_changes_during_the_session_is_not_written_with_the_old_bitmap() {
        let root = TempDir::new("session");
        let key = root.exe("app.exe", b"MZ version 1");
        let path = source_file(&key).unwrap().to_path_buf();
        // Like the platform: the first bitmap it loads for a key is what
        // it returns for the rest of the session.
        let loader = Loader::new();
        let platform = || loader.answer(Ok(Some(png(1, 100))));

        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        cache.get_or_load(&key, 48, platform).unwrap();
        cache.flush();
        // The app updates itself while Stem runs.
        fs::write(&path, b"MZ version 2, longer").unwrap();
        assert_eq!(
            cache.get_or_load(&key, 48, platform).unwrap(),
            Some(png(1, 100))
        );
        cache.flush();
        assert_eq!(cache.stats().writes, 1, "the stale bitmap is not written");
        drop(cache);

        // The next start loads the new icon and keeps it.
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        let fresh = || loader.answer(Ok(Some(png(2, 100))));
        assert_eq!(
            cache.get_or_load(&key, 48, fresh).unwrap(),
            Some(png(2, 100))
        );
        cache.flush();
        drop(cache);
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        let calls = loader.calls();
        assert_eq!(
            cache.get_or_load(&key, 48, fresh).unwrap(),
            Some(png(2, 100))
        );
        assert_eq!(loader.calls(), calls);
    }

    #[test]
    fn a_file_that_changes_during_its_load_is_never_written_this_session() {
        let root = TempDir::new("during");
        let key = root.exe("app.exe", b"MZ version 1");
        let path = source_file(&key).unwrap().to_path_buf();
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        let loader = Loader::new();
        cache
            .get_or_load(&key, 48, || {
                fs::write(&path, b"MZ version 2, longer").unwrap();
                loader.answer(Ok(Some(png(1, 100))))
            })
            .unwrap();
        // The platform now holds a bitmap of unknown vintage for the key.
        cache
            .get_or_load(&key, 48, || loader.answer(Ok(Some(png(1, 100)))))
            .unwrap();
        cache.flush();
        assert_eq!(cache.stats().writes, 0);
        assert_eq!(loader.calls(), 2);
        assert!(entries(&cache_dir(&root)).is_empty());
    }

    #[test]
    fn later_requests_in_a_session_touch_no_file() {
        let root = TempDir::new("repeats");
        let hit = root.exe("hit.exe", b"MZ hit");
        let loaded = root.exe("loaded.exe", b"MZ loaded");
        let dir = cache_dir(&root);
        {
            let cache = DiskIconCache::open(&dir).unwrap();
            cache
                .get_or_load(&hit, 48, || Ok(Some(png(1, 100))))
                .unwrap();
            cache.flush();
        }
        let cache = DiskIconCache::open(&dir).unwrap();
        let loader = Loader::new();
        let from_disk = || loader.answer(Ok(None));
        let from_platform = || loader.answer(Ok(Some(png(2, 100))));
        assert_eq!(
            cache.get_or_load(&hit, 48, from_disk).unwrap(),
            Some(png(1, 100))
        );
        assert_eq!(
            cache.get_or_load(&loaded, 48, from_platform).unwrap(),
            Some(png(2, 100))
        );
        cache.flush();
        assert_eq!(loader.calls(), 1);

        // Without the source files and the cache directory the session
        // answers as before: a stamp would now fail (bypassed) and a read
        // miss. The hit comes from its bytes, the loaded key from the
        // platform (its memory cache, on Windows).
        fs::remove_file(source_file(&hit).unwrap()).unwrap();
        fs::remove_file(source_file(&loaded).unwrap()).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        for _ in 0..3 {
            assert_eq!(
                cache.get_or_load(&hit, 48, from_disk).unwrap(),
                Some(png(1, 100))
            );
            assert_eq!(
                cache.get_or_load(&loaded, 48, from_platform).unwrap(),
                Some(png(2, 100))
            );
        }
        assert_eq!(loader.calls(), 4, "the hit never reaches the platform");
        cache.flush();
        let stats = cache.stats();
        assert_eq!(
            (stats.hits, stats.misses, stats.repeats, stats.bypassed),
            (1, 1, 6, 0)
        );
        assert_eq!((stats.writes, stats.write_failures), (1, 0));
    }

    #[test]
    fn of_two_racing_loads_only_the_first_is_written() {
        let root = TempDir::new("race");
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();

        // Another request for the same icon arrives while this one loads
        // and finishes first: one write, not two.
        let same = root.exe("same.exe", b"MZ");
        let got = cache
            .get_or_load(&same, 48, || {
                let inner = cache.get_or_load(&same, 48, || Ok(Some(png(1, 100))));
                assert_eq!(inner.unwrap(), Some(png(1, 100)));
                Ok(Some(png(1, 100)))
            })
            .unwrap();
        assert_eq!(got, Some(png(1, 100)));
        cache.flush();
        assert_eq!(cache.stats().writes, 1);

        // The app updates itself between the two: the racing request loads
        // the new file and is written; the first one's older bitmap is not.
        let updated = root.exe("updated.exe", b"MZ version 1");
        let path = source_file(&updated).unwrap().to_path_buf();
        let got = cache
            .get_or_load(&updated, 48, || {
                fs::write(&path, b"MZ version 2, longer").unwrap();
                let inner = cache.get_or_load(&updated, 48, || Ok(Some(png(2, 100))));
                assert_eq!(inner.unwrap(), Some(png(2, 100)));
                Ok(Some(png(1, 100)))
            })
            .unwrap();
        assert_eq!(got, Some(png(1, 100)));
        cache.flush();
        assert_eq!(cache.stats().writes, 2);
        drop(cache);
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        assert_eq!(
            cache.get_or_load(&updated, 48, || Ok(None)).unwrap(),
            Some(png(2, 100))
        );
    }

    #[test]
    fn keys_that_change_on_their_own_are_never_cached() {
        let root = TempDir::new("uncached");
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        let mock = MockPlatform::new();
        let loader = Loader::new();
        let lnk = root.path().join("app.lnk");
        fs::write(&lnk, b"shortcut").unwrap();
        let keys = [
            IconKey("hwnd:1234:56".into()),
            IconKey::shell("recycle-bin-full"),
            IconKey("aumid:Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
            IconKey("mock:exe:x".into()),
            IconKey(format!("file:{}", lnk.display())),
        ];
        for key in &keys {
            for _ in 0..2 {
                let expected = mock.icon_png(key, 48).unwrap();
                let got = cache
                    .get_or_load(key, 48, || loader.answer(mock.icon_png(key, 48)))
                    .unwrap();
                assert_eq!(got, expected);
            }
        }
        cache.flush();
        assert_eq!(loader.calls(), keys.len() * 2);
        assert_eq!(cache.stats().bypassed, 10);
        assert!(entries(&cache_dir(&root)).is_empty());
    }

    #[test]
    fn errors_none_and_missing_files_pass_through_and_write_nothing() {
        let root = TempDir::new("errors");
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        let key = root.exe("app.exe", b"MZ");
        let err = cache.get_or_load(&key, 48, || Err(PlatformError::Timeout));
        assert!(matches!(err, Err(PlatformError::Timeout)));
        assert_eq!(cache.get_or_load(&key, 48, || Ok(None)).unwrap(), None);
        let gone = IconKey(format!("exe:{}", root.path().join("gone.exe").display()));
        let loader = Loader::new();
        for _ in 0..2 {
            let got = cache
                .get_or_load(&gone, 48, || loader.answer(Ok(Some(png(1, 50)))))
                .unwrap();
            assert_eq!(got, Some(png(1, 50)));
        }
        assert_eq!(loader.calls(), 2);
        // Too large to cache: returned, not written.
        let huge = png(2, MAX_PAYLOAD_BYTES + 1);
        let other = root.exe("big.exe", b"MZ");
        assert_eq!(
            cache
                .get_or_load(&other, 48, || Ok(Some(huge.clone())))
                .unwrap(),
            Some(huge)
        );
        cache.flush();
        assert_eq!(cache.stats().writes, 0);
        assert!(entries(&cache_dir(&root)).is_empty());
    }

    #[test]
    fn a_corrupt_or_foreign_entry_is_a_miss_and_is_replaced() {
        let root = TempDir::new("corrupt");
        let key = root.exe("app.exe", b"MZ");
        let name = entry_name(&key, 48);
        let file = cache_dir(&root).join(&name);
        let good = || -> Vec<u8> {
            let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
            cache
                .get_or_load(&key, 48, || Ok(Some(png(1, 300))))
                .unwrap();
            cache.flush();
            fs::read(&file).unwrap()
        };
        let bytes = good();
        let stamp = Stamp::of(source_file(&key).unwrap()).unwrap();

        let mut flipped = bytes.clone();
        flipped[bytes.len() / 2] ^= 0xff;
        let build = build_id().unwrap();
        let other_key = encode(&build, "exe:C:/other.exe", 48, stamp, &png(5, 300));
        let other_size = encode(&build, &key.0, 96, stamp, &png(5, 300));
        let other_version = encode("0.0.0-old", &key.0, 48, stamp, &png(5, 300));
        // What the cache's first version wrote: the crate version alone.
        let version_only = encode(CRATE_VERSION, &key.0, 48, stamp, &png(5, 300));
        let other_build = encode(
            &format!("{CRATE_VERSION}+1.2"),
            &key.0,
            48,
            stamp,
            &png(5, 300),
        );
        let cases: [(&str, Vec<u8>, bool); 9] = [
            ("empty", Vec::new(), true),
            ("truncated", bytes[..bytes.len() - 3].to_vec(), true),
            ("flipped", flipped, true),
            ("garbage", vec![0xAB; 5000], true),
            ("another key's", other_key, false),
            ("another size's", other_size, false),
            ("another version's", other_version, false),
            ("a version-only producer's", version_only, false),
            ("another build's", other_build, false),
        ];
        for (what, contents, corrupt) in cases {
            fs::write(&file, &contents).unwrap();
            let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
            let loader = Loader::new();
            let got = cache
                .get_or_load(&key, 48, || loader.answer(Ok(Some(png(2, 300)))))
                .unwrap();
            assert_eq!(got, Some(png(2, 300)), "{what}");
            assert_eq!(loader.calls(), 1, "{what} entry must miss");
            cache.flush();
            let stats = cache.stats();
            assert_eq!(
                (stats.hits, stats.misses, stats.writes),
                (0, 1, 1),
                "{what}"
            );
            drop(cache);
            // Replaced by a valid entry for the key (a corrupt one was
            // deleted first).
            let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
            let got = cache.get_or_load(&key, 48, || Ok(None)).unwrap();
            assert_eq!(
                got,
                Some(png(2, 300)),
                "{what} not replaced (corrupt: {corrupt})"
            );
        }

        // An entry the cache cannot open is a miss too: a directory where
        // the file should be.
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        let got = cache
            .get_or_load(&key, 48, || Ok(Some(png(3, 300))))
            .unwrap();
        assert_eq!(got, Some(png(3, 300)));
        cache.flush();
        assert_eq!(cache.stats().write_failures, 1);
    }

    fn set_age(path: &Path, seconds_ago: u64) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(seconds_ago))
            .unwrap();
    }

    #[test]
    fn the_file_bound_evicts_the_least_recently_used_entry() {
        let root = TempDir::new("files");
        let limits = CacheLimits {
            max_bytes: DEFAULT_MAX_BYTES,
            max_files: 3,
        };
        let keys: Vec<IconKey> = (0..4)
            .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
            .collect();
        {
            let cache = DiskIconCache::open_with_limits(cache_dir(&root), limits).unwrap();
            for (i, key) in keys.iter().take(3).enumerate() {
                cache
                    .get_or_load(key, 48, || Ok(Some(png(i as u8, 200))))
                    .unwrap();
                cache.flush();
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        // The next start uses app0 again (a disk hit), so app1 is now the
        // least recently used.
        let cache = DiskIconCache::open_with_limits(cache_dir(&root), limits).unwrap();
        cache.get_or_load(&keys[0], 48, || Ok(None)).unwrap();
        cache.flush();
        std::thread::sleep(Duration::from_millis(5));
        cache
            .get_or_load(&keys[3], 48, || Ok(Some(png(3, 200))))
            .unwrap();
        cache.flush();

        let stats = cache.stats();
        assert_eq!((stats.hits, stats.entries, stats.evictions), (1, 3, 1));
        let mut expected: Vec<String> = [0, 2, 3]
            .iter()
            .map(|&i| entry_name(&keys[i], 48))
            .collect();
        expected.sort();
        assert_eq!(entries(&cache_dir(&root)), expected);
    }

    #[test]
    fn the_byte_bound_evicts_oldest_first_and_never_the_entry_just_written() {
        let root = TempDir::new("bytes");
        let one = u64::try_from(
            encode(
                &build_id().unwrap(),
                "exe:",
                48,
                Stamp {
                    len: 0,
                    modified_ns: 0,
                },
                &png(0, 1000),
            )
            .len(),
        )
        .unwrap();
        let keys: Vec<IconKey> = (0..5)
            .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
            .collect();
        // Room for two and a half entries (keys differ in length by nothing).
        let key_len = u64::try_from(keys[0].0.len() - "exe:".len()).unwrap();
        let limits = CacheLimits {
            max_bytes: (one + key_len) * 5 / 2,
            max_files: DEFAULT_MAX_FILES,
        };
        let cache = DiskIconCache::open_with_limits(cache_dir(&root), limits).unwrap();
        for (i, key) in keys.iter().enumerate() {
            cache
                .get_or_load(key, 48, || Ok(Some(png(i as u8, 1000))))
                .unwrap();
            cache.flush();
            std::thread::sleep(Duration::from_millis(5));
        }
        let stats = cache.stats();
        assert_eq!(stats.entries, 2);
        assert_eq!(stats.evictions, 3);
        assert!(stats.bytes <= limits.max_bytes, "{stats:?}");
        let mut expected: Vec<String> = [3, 4].iter().map(|&i| entry_name(&keys[i], 48)).collect();
        expected.sort();
        assert_eq!(entries(&cache_dir(&root)), expected);

        // One entry larger than the whole bound is kept (it was just
        // written) and evicts everything else.
        let tiny = CacheLimits {
            max_bytes: 10,
            max_files: DEFAULT_MAX_FILES,
        };
        drop(cache);
        let cache = DiskIconCache::open_with_limits(cache_dir(&root), tiny).unwrap();
        cache.flush();
        assert_eq!(
            cache.stats().entries,
            0,
            "the start evicts down to the bound"
        );
        cache
            .get_or_load(&keys[0], 48, || Ok(Some(png(9, 1000))))
            .unwrap();
        cache.flush();
        assert_eq!(cache.stats().entries, 1);
    }

    #[test]
    fn a_start_indexes_by_recency_evicts_to_the_bounds_and_removes_stray_temp_files() {
        let root = TempDir::new("scan");
        let keys: Vec<IconKey> = (0..3)
            .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
            .collect();
        {
            let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
            for (i, key) in keys.iter().enumerate() {
                cache
                    .get_or_load(key, 48, || Ok(Some(png(i as u8, 200))))
                    .unwrap();
            }
            cache.flush();
        }
        let dir = cache_dir(&root);
        // app1 was used most recently, app0 least.
        set_age(&dir.join(entry_name(&keys[0], 48)), 3000);
        set_age(&dir.join(entry_name(&keys[1], 48)), 10);
        set_age(&dir.join(entry_name(&keys[2], 48)), 2000);
        let stray = dir.join(temp_name(&entry_name(&keys[0], 48), 7));
        fs::write(&stray, b"half an entry").unwrap();
        let foreign = dir.join("notes.txt");
        fs::write(&foreign, b"not ours").unwrap();

        let limits = CacheLimits {
            max_bytes: DEFAULT_MAX_BYTES,
            max_files: 2,
        };
        let cache = DiskIconCache::open_with_limits(&dir, limits).unwrap();
        cache.flush();
        assert!(!stray.exists());
        assert!(
            foreign.exists(),
            "files that are not entries are left alone"
        );
        let mut expected: Vec<String> = [1, 2].iter().map(|&i| entry_name(&keys[i], 48)).collect();
        expected.sort();
        assert_eq!(entries(&dir), expected);
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn a_hit_on_an_old_entry_saves_its_recency_for_the_next_start() {
        let root = TempDir::new("touch");
        let key = root.exe("app.exe", b"MZ");
        let dir = cache_dir(&root);
        {
            let cache = DiskIconCache::open(&dir).unwrap();
            cache
                .get_or_load(&key, 48, || Ok(Some(png(1, 200))))
                .unwrap();
        }
        let file = dir.join(entry_name(&key, 48));
        let old = TOUCH_PERSIST_AFTER.as_secs() + 60;
        set_age(&file, old);
        let before = fs::metadata(&file).unwrap().modified().unwrap();
        let cache = DiskIconCache::open(&dir).unwrap();
        assert!(cache.get_or_load(&key, 48, || Ok(None)).unwrap().is_some());
        cache.flush();
        let after = fs::metadata(&file).unwrap().modified().unwrap();
        assert!(
            after.duration_since(before).unwrap() >= Duration::from_secs(old - 5),
            "{before:?} -> {after:?}"
        );
        // A recent entry is not rewritten on a hit (the next start's).
        drop(cache);
        let cache = DiskIconCache::open(&dir).unwrap();
        assert!(cache.get_or_load(&key, 48, || Ok(None)).unwrap().is_some());
        cache.flush();
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), after);
    }

    #[test]
    fn a_cache_directory_that_goes_away_is_recreated_by_the_next_write() {
        let root = TempDir::new("gone");
        let dir = cache_dir(&root);
        let cache = DiskIconCache::open(&dir).unwrap();
        // Once the writer's start-up scan is done with the directory.
        cache.flush();
        fs::remove_dir_all(&dir).unwrap();
        let key = root.exe("app.exe", b"MZ");
        cache
            .get_or_load(&key, 48, || Ok(Some(png(1, 200))))
            .unwrap();
        cache.flush();
        assert_eq!(cache.stats().writes, 1);
        assert_eq!(entries(&dir).len(), 1);
    }

    #[test]
    fn open_fails_where_no_directory_can_be() {
        let root = TempDir::new("open");
        let file = root.path().join("a-file");
        fs::write(&file, b"x").unwrap();
        assert!(DiskIconCache::open(&file).is_err());
        assert!(DiskIconCache::open(file.join("below")).is_err());
    }

    #[test]
    fn shutdown_drains_the_queue_and_later_misses_still_answer() {
        // Few enough writes to fit the drain budget on a loaded machine.
        const KEYS: usize = 8;
        let root = TempDir::new("shutdown");
        let keys: Vec<IconKey> = (0..KEYS)
            .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
            .collect();
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        for (i, key) in keys.iter().enumerate() {
            cache
                .get_or_load(key, 48, || Ok(Some(png(i as u8, 2000))))
                .unwrap();
        }
        cache.shutdown();
        cache.shutdown();
        let stats = cache.stats();
        assert_eq!((stats.writes, stats.abandoned), (KEYS as u64, 0));
        assert_eq!(entries(&cache_dir(&root)).len(), KEYS);
        // After shutdown reads still work; nothing more is written.
        let extra = root.exe("extra.exe", b"MZ");
        assert_eq!(
            cache
                .get_or_load(&extra, 48, || Ok(Some(png(1, 10))))
                .unwrap(),
            Some(png(1, 10))
        );
        // A key this session loaded goes to the platform, as before.
        assert_eq!(
            cache
                .get_or_load(&keys[0], 48, || Ok(Some(png(0, 2000))))
                .unwrap(),
            Some(png(0, 2000))
        );
        cache.flush();
        assert_eq!(entries(&cache_dir(&root)).len(), KEYS);
    }

    #[test]
    fn shutdown_waits_for_a_stuck_writer_only_so_long_and_its_queue_is_dropped() {
        let root = TempDir::new("stuck");
        let keys: Vec<IconKey> = (0..5)
            .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
            .collect();
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        cache.flush();
        // A file operation held up for 3 s (a file system filter, say),
        // with writes queued behind it.
        cache.tx.send(Job::Stall(Duration::from_secs(3))).unwrap();
        for (i, key) in keys.iter().enumerate() {
            cache
                .get_or_load(key, 48, || Ok(Some(png(i as u8, 200))))
                .unwrap();
        }
        let started = Instant::now();
        cache.shutdown();
        let took = started.elapsed();
        assert!(
            took >= SHUTDOWN_WAIT && took < Duration::from_millis(2000),
            "shutdown took {took:?}"
        );
        // Reads still answer while the writer is stuck.
        assert_eq!(
            cache
                .get_or_load(&keys[0], 48, || Ok(Some(png(0, 200))))
                .unwrap(),
            Some(png(0, 200))
        );

        // Once free, the writer drops what was queued instead of writing
        // it, and ends.
        let deadline = Instant::now() + Duration::from_secs(30);
        while cache.stats().abandoned < 5 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let stats = cache.stats();
        assert_eq!((stats.abandoned, stats.writes), (5, 0));
        assert!(entries(&cache_dir(&root)).is_empty());
        let deadline = Instant::now() + Duration::from_secs(30);
        assert!(
            !matches!(
                cache.writer_stopped.recv_deadline(deadline),
                Err(RecvTimeoutError::Timeout)
            ),
            "the writer never ended"
        );
    }
}
