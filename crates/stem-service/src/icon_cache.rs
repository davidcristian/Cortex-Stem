//! Disk cache for icon bitmaps, behind [`crate::DockService::icon_png`].
//!
//! The platform turns an [`IconKey`] into PNG bytes and keeps what it made
//! in memory for the session (`stem_platform::windows::icons::IconCache`).
//! This cache keeps the bytes across sessions, so a warm start reads the
//! dock's icons from disk instead of extracting every one again.
//!
//! **What is cached.** Only keys whose bitmap is a pure function of one file
//! on disk ([`source_file`]; but see the limitation below): `exe:<path>`
//! and `file:<path>` where `path` is
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
//! **Known limitation: fallback bitmaps.** The platform's answer does not
//! say which of its chains produced it. For a PE image whose resources it
//! cannot read (the mapping fails, no `RT_GROUP_ICON`, a malformed
//! directory) it falls back to the shell: `IShellItemImageFactory`, then
//! `PrivateExtractIconsW`, then `SHGetFileInfoW`. That bitmap depends on the
//! shell as well as on the file (the shell's generic exe icon, its own
//! resampling, an `E_PENDING` retry that failed over), yet it is cached
//! like a `PE resources` one, until the exe changes, another build of Stem
//! runs or the entry is evicted; without this cache it lasts one session. The
//! platform's debug line for each real load names the chain (`via PE
//! resources`, `via IShellItemImageFactory`, …), which tells such entries
//! apart in a cold start's log. Writing only `PE resources` results needs a
//! platform signal (the platform's memory cache would have to keep the
//! chain with the bytes).
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

mod format;
mod index;
mod lifecycle;
mod lookup;
mod source;
mod stats;
#[cfg(test)]
pub(crate) mod tests;
mod writer;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use stem_core::IconKey;

pub use source::{Stamp, build_id, source_file};
pub use stats::CacheStats;

use lookup::Seen;
use stats::Counters;

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
