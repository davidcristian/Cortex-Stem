//! Display names: the cleaning every name passes through, the fallbacks
//! when nothing better is known, and the version resource strings they are
//! read from.

use super::paths::file_stem;

/// First candidate that is non-empty after trimming.
pub fn first_non_empty<'a, I>(candidates: I) -> Option<String>
where
    I: IntoIterator<Item = Option<&'a str>>,
{
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The name shown when nothing better is known.
pub const PLACEHOLDER_DISPLAY_NAME: &str = "Application";

/// A display name fit to show: trimmed, with every trailing `.exe`
/// (case-insensitive) removed, and non-empty. `None` when nothing is left.
/// Every name the identity code produces passes through here, so no item is
/// ever titled `Notepad.exe`: the file name of an unreadable
/// executable, a window title that is a program name, a shortcut called
/// `app.exe.lnk` and a version resource that repeats the file name are all
/// cleaned the same way.
pub fn clean_display_name(name: &str) -> Option<String> {
    let mut name = name.trim();
    while let Some(stripped) = strip_exe_suffix(name) {
        name = stripped.trim_end();
    }
    (!name.is_empty()).then(|| name.to_owned())
}

/// `name` without one trailing `.exe` (any case); `None` when it has none.
fn strip_exe_suffix(name: &str) -> Option<&str> {
    let cut = name.len().checked_sub(4)?;
    let (head, tail) = name.split_at_checked(cut)?;
    tail.eq_ignore_ascii_case(".exe").then_some(head)
}

/// First candidate that [`clean_display_name`] accepts.
pub fn first_display_name<'a, I>(candidates: I) -> Option<String>
where
    I: IntoIterator<Item = Option<&'a str>>,
{
    candidates
        .into_iter()
        .flatten()
        .find_map(clean_display_name)
}

/// Display name when no version resource is available: exe stem, then the
/// window title, then the class name, then a fixed placeholder; each
/// candidate cleaned by [`clean_display_name`].
pub fn display_name_fallback(exe_path: Option<&str>, title: &str, class_name: &str) -> String {
    let stem = exe_path.map(file_stem);
    first_display_name([stem.as_deref(), Some(title), Some(class_name)])
        .unwrap_or_else(|| PLACEHOLDER_DISPLAY_NAME.to_owned())
}

/// A readable fallback name for an AppUserModelID when the shell cannot
/// resolve it: `Microsoft.WindowsCalculator_8wekyb3d8bbwe!App` →
/// `WindowsCalculator`, `Chrome.Profile1` → `Profile1`, `Chrome` → `Chrome`.
pub fn aumid_fallback_name(aumid: &str) -> String {
    let Some(aumid) = clean_display_name(aumid) else {
        return PLACEHOLDER_DISPLAY_NAME.to_owned();
    };
    let family = aumid.split('!').next().unwrap_or(&aumid);
    let name = family.split('_').next().unwrap_or(family);
    let last = name.rsplit('.').next().unwrap_or(name);
    let chosen = if last.is_empty() { &aumid } else { last };
    first_display_name([Some(chosen), Some(&aumid)])
        .unwrap_or_else(|| PLACEHOLDER_DISPLAY_NAME.to_owned())
}

/// Sub-block path for `VerQueryValueW`: `\StringFileInfo\{lang}{codepage}\{name}`.
pub fn version_string_path(lang: u16, codepage: u16, name: &str) -> String {
    format!(r"\StringFileInfo\{lang:04x}{codepage:04x}\{name}")
}

/// A string value of a version block, from the bytes `VerQueryValueW`
/// points at: `len` UTF-16 units, little-endian (the NUL included or
/// not), cut to what `bytes` holds, decoded up to the first NUL.
pub fn version_string_value(bytes: &[u8], len: usize) -> String {
    let units = len.min(bytes.len() / 2);
    let wide: Vec<u16> = bytes[..units * 2]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    crate::rules::wide::from_wide(&wide)
}

/// Parse the `\VarFileInfo\Translation` block: an array of
/// `(language, code page)` 16-bit pairs, little-endian. Trailing bytes that
/// do not form a pair are ignored.
pub fn translations(block: &[u8]) -> Vec<(u16, u16)> {
    block
        .chunks_exact(4)
        .map(|c| {
            (
                u16::from_le_bytes([c[0], c[1]]),
                u16::from_le_bytes([c[2], c[3]]),
            )
        })
        .collect()
}

/// Translations to try, in order: the file's own first, then the usual
/// suspects (US English in Unicode and in the Windows-1252 code page, and the
/// neutral language), without duplicates.
pub fn translation_candidates(declared: &[(u16, u16)]) -> Vec<(u16, u16)> {
    let mut out: Vec<(u16, u16)> = Vec::with_capacity(declared.len() + 3);
    for t in declared
        .iter()
        .copied()
        .chain([(0x0409, 0x04B0), (0x0409, 0x04E4), (0x0000, 0x04B0)])
    {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Version-resource string names consulted for a display name, best first.
pub const VERSION_NAME_FIELDS: [&str; 2] = ["FileDescription", "ProductName"];
