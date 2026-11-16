//! Which keys are cached ([`source_file`]), and what decides whether an
//! entry is still valid ([`Stamp`], [`build_id`]).

use std::fs;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use stem_core::IconKey;
use stem_platform::rules::icons::is_pe_image_path;

use super::{CRATE_VERSION, MAX_KEY_BYTES};

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
