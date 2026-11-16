//! The disk icon cache in temporary directories. The shared helpers are
//! here (`perf_tests` uses [`TempDir`] and [`png`]); each submodule tests
//! one part.

mod bounds;
mod format;
mod lifecycle;
mod lookup;

use std::fs::{self, File};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use crossbeam_channel::RecvTimeoutError;
use stem_platform::mock::MockPlatform;
use stem_platform::{Platform, PlatformError, PlatformResult};

use super::format::{Decoded, decode, encode, entry_name, is_entry_name, is_temp_name, temp_name};
use super::*;

/// A fresh directory under the system temp dir, removed on drop.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new(tag: &str) -> Self {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("stem-icon-cache-{tag}-{}-{n}", std::process::id()));
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

    fn answer(&self, answer: PlatformResult<Option<Vec<u8>>>) -> PlatformResult<Option<Vec<u8>>> {
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
