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

use std::collections::HashSet;
use std::fmt;

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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn strings(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|&p| p.to_owned()).collect()
    }

    fn everything_exists(paths: &[&str]) -> RecyclePlan {
        plan_recycle(&strings(paths), |_| true)
    }

    fn skipped(path: &str, reason: SkipReason) -> Skipped {
        Skipped {
            path: path.to_owned(),
            reason,
        }
    }

    #[test]
    fn keeps_drive_and_unc_paths_in_the_shells_form() {
        let plan = everything_exists(&[
            r"C:\Users\me\Desktop\notes.txt",
            "C:/Users/me/Desktop/Folder/",
            r"d:\Data\\archive.zip",
            r"\\nas\share\photos\2024",
            "//nas/share/report.pdf",
        ]);
        assert_eq!(
            plan.recycle,
            strings(&[
                r"C:\Users\me\Desktop\notes.txt",
                r"C:\Users\me\Desktop\Folder",
                r"d:\Data\archive.zip",
                r"\\nas\share\photos\2024",
                r"\\nas\share\report.pdf",
            ])
        );
        assert_eq!(plan.skipped, vec![]);
    }

    #[test]
    fn leaves_out_what_is_not_a_plain_absolute_file_path() {
        let odd = [
            "",
            "notes.txt",
            r"Documents\notes.txt",
            r"\Users\me\notes.txt",
            "C:notes.txt",
            "C:",
            r"\\?\C:\Users\me\notes.txt",
            r"\\.\pipe\stem",
            r"\\?\UNC\nas\share\x",
            r"C:\Users\me\..\..\Windows",
            r"C:\Users\.\me",
            r"C:\Users\me\*.txt",
            r"C:\Users\me\notes?.txt",
            r"C:\Users\me\notes.txt:secret",
            "C:\\Users\\me\0\\notes.txt",
            r"\\\nas\share\x",
            "::{645FF040-5081-101B-9F08-00AA002F954E}",
            "shell:RecycleBinFolder",
        ];
        let plan = everything_exists(&odd);
        assert_eq!(plan.recycle, Vec::<String>::new());
        assert_eq!(
            plan.skipped,
            odd.iter()
                .map(|p| skipped(p, SkipReason::NotAFilePath))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn never_asks_for_a_drive_or_share_root() {
        let roots = [
            r"C:\",
            "C:/",
            r"C:\\",
            r"\\nas",
            r"\\nas\",
            r"\\nas\share",
            r"\\nas\share\",
        ];
        let plan = everything_exists(&roots);
        assert_eq!(plan.recycle, Vec::<String>::new());
        let reasons: Vec<SkipReason> = plan.skipped.iter().map(|s| s.reason).collect();
        assert_eq!(
            reasons,
            vec![
                SkipReason::Root,
                SkipReason::Root,
                SkipReason::Root,
                SkipReason::Root,
                SkipReason::Root,
                SkipReason::Root,
                SkipReason::Root,
            ]
        );
    }

    #[test]
    fn asks_whether_each_plain_path_exists_in_the_shells_form() {
        let asked = std::cell::RefCell::new(Vec::new());
        let plan = plan_recycle(
            &strings(&["C:/a/here.txt", "C:/a/gone.txt", "relative.txt"]),
            |p| {
                asked.borrow_mut().push(p.to_owned());
                !p.ends_with("gone.txt")
            },
        );
        assert_eq!(
            *asked.borrow(),
            strings(&[r"C:\a\here.txt", r"C:\a\gone.txt"])
        );
        assert_eq!(plan.recycle, strings(&[r"C:\a\here.txt"]));
        assert_eq!(
            plan.skipped,
            vec![
                skipped("C:/a/gone.txt", SkipReason::Missing),
                skipped("relative.txt", SkipReason::NotAFilePath),
            ]
        );
    }

    #[test]
    fn lists_each_path_once_and_lets_a_folder_take_its_contents_along() {
        let plan = everything_exists(&[
            r"C:\Work\report.docx",
            r"C:\Work\Drafts\one.txt",
            "c:/work/REPORT.DOCX",
            r"C:\Work\Drafts",
            r"C:\Work\Drafts2\two.txt",
            r"C:\Work\Drafts\",
        ]);
        assert_eq!(
            plan.recycle,
            strings(&[
                r"C:\Work\report.docx",
                r"C:\Work\Drafts",
                r"C:\Work\Drafts2\two.txt"
            ])
        );
        assert_eq!(
            plan.skipped,
            vec![
                skipped(r"C:\Work\Drafts\one.txt", SkipReason::InsideDropped),
                skipped("c:/work/REPORT.DOCX", SkipReason::Duplicate),
                skipped(r"C:\Work\Drafts\", SkipReason::Duplicate),
            ]
        );
    }

    #[test]
    fn only_refusals_and_missing_paths_are_warnings() {
        assert!(SkipReason::NotAFilePath.is_warning());
        assert!(SkipReason::Root.is_warning());
        assert!(SkipReason::Missing.is_warning());
        assert!(!SkipReason::Duplicate.is_warning());
        assert!(!SkipReason::InsideDropped.is_warning());
    }

    #[test]
    fn says_why_nothing_goes_to_the_bin() {
        let none = |paths: &[&str], exists: bool| {
            nothing_to_recycle(&plan_recycle(&strings(paths), |_| exists))
        };
        assert_eq!(
            nothing_to_recycle(&RecyclePlan::default()),
            "no paths to move to the Recycle Bin"
        );
        assert_eq!(
            none(&[r"C:\Users\me\notes.txt"], false),
            r"C:\Users\me\notes.txt does not exist"
        );
        assert_eq!(none(&[r"C:\"], true), r"C:\ is a drive or share root");
        assert_eq!(
            none(&["notes.txt"], true),
            "notes.txt is not an absolute file path"
        );
        assert_eq!(
            none(&[r"C:\a.txt", r"C:\b.txt", r"c:\A.TXT"], false),
            "none of the 3 paths exists"
        );
        assert_eq!(
            none(&[r"C:\a.txt", "b.txt"], false),
            "none of the 2 paths can be moved to the Recycle Bin"
        );
    }
}
