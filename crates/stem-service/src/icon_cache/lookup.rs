//! The read path of [`DiskIconCache::get_or_load`]: this session's memory,
//! then the disk entry, then the platform, whose answer is queued for
//! writing.

use std::collections::hash_map::Entry;
use std::fs::File;
use std::io::{self, Read};

use stem_core::IconKey;
use stem_platform::PlatformResult;

use super::format::{decode, entry_name};
use super::stats::Counters;
use super::{DiskIconCache, Job, MAX_ENTRY_BYTES, MAX_PAYLOAD_BYTES, Stamp, source_file};

/// What this session already did with a cacheable (key, size).
#[derive(Debug)]
pub(super) enum Seen {
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

impl DiskIconCache {
    /// `key`'s bitmap at `size`: from disk when a valid entry exists, else
    /// from `load` (the platform), whose `Some` result is queued for
    /// writing when the key is cacheable and the source file did not change
    /// meanwhile. A (key, size) this session already answered touches no
    /// file (the `icon_cache` module docs, "Within a session"). `load`'s
    /// errors are returned unchanged; the cache itself never fails.
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
}
