//! Which dropped paths go to the Recycle Bin.
//!
//! A file drop on the Recycle Bin item hands the platform every path of the
//! drag. Before the shell is asked, [`plan_recycle`] sorts them (pure, so it
//! is tested on every host; whether a path exists is the caller's answer):
//!
//! 1. only plain absolute file paths: a drive path (`C:\…`) or a UNC path
//!    (`\\server\share\…`), with `\` or `/` separators; no device or
//!    verbatim prefix (`\\?\`, `\\.\`), no `.` or `..` component, no
//!    wildcard, no `:` past the drive (an alternate data stream), no NUL;
//! 2. never a drive root or a share root;
//! 3. only what exists;
//! 4. each path once (letter case and separators do not matter), and
//!    nothing inside another dropped folder: it goes along with the folder,
//!    and asking for it again would fail once the folder has moved.
//!
//! The paths kept are handed on with `\` separators, no doubled or trailing
//! separator, in drop order.
//!
//! [`bin_state`] turns what `SHQueryRecycleBinW` reports into the bin's state.

use std::collections::HashSet;
use std::fmt;

use stem_core::RecycleBinState;

/// Why a dropped path does not go to the bin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Not a plain absolute file path (see the module docs).
    NotAFilePath,
    /// A drive root (`C:\`) or a share root (`\\server\share`).
    Root,
    /// Nothing at that path.
    Missing,
    /// The same path came earlier in the drop.
    Duplicate,
    /// Inside another dropped folder, which takes it along.
    InsideDropped,
}

impl SkipReason {
    /// Whether the user asked for something that will not happen (a
    /// warning), rather than for something that happens anyway (a
    /// duplicate, a file inside a dropped folder).
    pub fn is_warning(self) -> bool {
        matches!(self, Self::NotAFilePath | Self::Root | Self::Missing)
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAFilePath => "is not an absolute file path",
            Self::Root => "is a drive or share root",
            Self::Missing => "does not exist",
            Self::Duplicate => "is listed twice",
            Self::InsideDropped => "is inside another dropped folder",
        })
    }
}

/// A dropped path left out, as it was dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: String,
    pub reason: SkipReason,
}

/// What a drop on the Recycle Bin asks the shell to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecyclePlan {
    /// The paths to recycle, in drop order, in the shell's form.
    pub recycle: Vec<String>,
    /// The paths left out, in drop order.
    pub skipped: Vec<Skipped>,
}

/// Sort the dropped `paths` (see the module docs). `exists` is asked about
/// each plain path, in the shell's form, before duplicates are dropped.
pub fn plan_recycle(paths: &[String], exists: impl Fn(&str) -> bool) -> RecyclePlan {
    // (drop index, shell form, lower-case key) of the paths still in.
    let mut kept: Vec<(usize, String, String)> = Vec::new();
    let mut keys: HashSet<String> = HashSet::new();
    let mut skipped: Vec<(usize, SkipReason)> = Vec::new();
    for (index, raw) in paths.iter().enumerate() {
        let shell = match shell_path(raw) {
            Ok(shell) => shell,
            Err(reason) => {
                skipped.push((index, reason));
                continue;
            }
        };
        if !exists(&shell) {
            skipped.push((index, SkipReason::Missing));
            continue;
        }
        let key = shell.to_lowercase();
        if !keys.insert(key.clone()) {
            skipped.push((index, SkipReason::Duplicate));
            continue;
        }
        kept.push((index, shell, key));
    }
    let mut recycle = Vec::with_capacity(kept.len());
    for (index, shell, key) in kept {
        if has_dropped_ancestor(&key, &keys) {
            skipped.push((index, SkipReason::InsideDropped));
        } else {
            recycle.push(shell);
        }
    }
    skipped.sort_by_key(|&(index, _)| index);
    RecyclePlan {
        recycle,
        skipped: skipped
            .into_iter()
            .map(|(index, reason)| Skipped {
                path: paths[index].clone(),
                reason,
            })
            .collect(),
    }
}

/// Why a plan recycles nothing: the error a drop on the bin reports then.
/// Meant for a plan whose `recycle` list is empty.
pub fn nothing_to_recycle(plan: &RecyclePlan) -> String {
    match plan.skipped.as_slice() {
        [] => "no paths to move to the Recycle Bin".to_owned(),
        [one] => format!("{} {}", one.path, one.reason),
        many if many.iter().all(|s| s.reason == SkipReason::Missing) => {
            format!("none of the {} paths exists", many.len())
        }
        many => format!(
            "none of the {} paths can be moved to the Recycle Bin",
            many.len()
        ),
    }
}

/// Whether a folder holding `key` is among the dropped `keys` (all
/// lower-case shell forms). Each `\` past the first character ends a
/// candidate folder; the roots among them are never in `keys`.
fn has_dropped_ancestor(key: &str, keys: &HashSet<String>) -> bool {
    key.match_indices('\\')
        .any(|(at, _)| at > 0 && keys.contains(&key[..at]))
}

/// The shell's form of a plain absolute file path, or why it is not one.
fn shell_path(raw: &str) -> Result<String, SkipReason> {
    if raw.is_empty() || raw.contains(['\0', '*', '?']) {
        return Err(SkipReason::NotAFilePath);
    }
    let path = raw.replace('/', "\\");
    let (head, tail) = if let Some(unc) = path.strip_prefix("\\\\") {
        let mut parts = unc.split('\\');
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        if server.is_empty() || !plain_component(server) {
            return Err(SkipReason::NotAFilePath);
        }
        if share.is_empty() {
            return Err(SkipReason::Root);
        }
        if !plain_component(share) {
            return Err(SkipReason::NotAFilePath);
        }
        (format!("\\\\{server}\\{share}"), parts.collect::<Vec<_>>())
    } else {
        let bytes = path.as_bytes();
        let is_drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\';
        if !is_drive {
            return Err(SkipReason::NotAFilePath);
        }
        (path[..2].to_owned(), path[3..].split('\\').collect())
    };
    let components: Vec<&str> = tail.into_iter().filter(|c| !c.is_empty()).collect();
    if components.is_empty() {
        return Err(SkipReason::Root);
    }
    if !components.iter().all(|c| plain_component(c)) {
        return Err(SkipReason::NotAFilePath);
    }
    Ok(format!("{head}\\{}", components.join("\\")))
}

/// A path component that names a file or folder: not `.` or `..`, no `:`.
fn plain_component(component: &str) -> bool {
    component != "." && component != ".." && !component.contains(':')
}

/// The bin's state from `SHQueryRecycleBinW`'s `i64NumItems` and
/// `i64Size` (every drive's bin); a negative value reads as 0.
pub fn bin_state(item_count: i64, size_bytes: i64) -> RecycleBinState {
    RecycleBinState {
        item_count: u64::try_from(item_count).unwrap_or(0),
        size_bytes: u64::try_from(size_bytes).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests;
