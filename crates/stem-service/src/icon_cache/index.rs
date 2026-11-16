//! The writer's index of the entries on disk, and the eviction that keeps
//! them within the bounds.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::format::{is_entry_name, is_temp_name};
use super::{CacheLimits, TOUCH_PERSIST_AFTER};

/// An indexed entry: its size on disk, when it was last used, and the
/// modification time its file carries (the recency a restart sees).
#[derive(Debug, Clone, Copy)]
pub(super) struct Slot {
    pub(super) len: u64,
    pub(super) used: SystemTime,
    pub(super) persisted: SystemTime,
}

#[derive(Debug, Default)]
pub(super) struct Index {
    pub(super) slots: HashMap<String, Slot>,
    pub(super) bytes: u64,
}

impl Index {
    /// Index the entries in `dir`, deleting stray temp files of earlier
    /// runs. Files that are neither are left alone and not counted.
    pub(super) fn scan(dir: &Path) -> Self {
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

    pub(super) fn insert(&mut self, name: String, slot: Slot) {
        self.bytes += slot.len;
        if let Some(old) = self.slots.insert(name, slot) {
            self.bytes -= old.len;
        }
    }

    pub(super) fn forget(&mut self, name: &str) {
        if let Some(old) = self.slots.remove(name) {
            self.bytes -= old.len;
        }
    }

    fn over(&self, limits: CacheLimits) -> bool {
        self.slots.len() > limits.max_files || self.bytes > limits.max_bytes
    }

    /// Delete least recently used entries (never `keep`) until the index
    /// is within `limits`. Returns how many were deleted.
    pub(super) fn evict(&mut self, dir: &Path, limits: CacheLimits, keep: Option<&str>) -> u64 {
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
    pub(super) fn touch(&mut self, dir: &Path, name: &str) {
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
