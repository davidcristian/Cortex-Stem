//! Icon keys: namespaced strings that name where an icon comes from, and
//! the `hwnd:` keys tied to one live window.

use stem_core::IconKey;

pub fn exe_icon_key(path: &str) -> IconKey {
    IconKey(format!("exe:{path}"))
}

pub fn file_icon_key(path: &str) -> IconKey {
    IconKey(format!("file:{path}"))
}

pub fn aumid_icon_key(aumid: &str) -> IconKey {
    IconKey(format!("aumid:{}", aumid.trim()))
}

/// The icon-key namespace of a window's own icon ([`hwnd_icon_key`]).
pub const WINDOW_ICON_NAMESPACE: &str = "hwnd";

/// Key of a window's own icon: `hwnd:<id>:<pid>`. The process id is part of
/// the key because HWND values are recycled by the OS; a key that outlives
/// its window must not resolve to a different window's icon.
pub fn hwnd_icon_key(id: u64, process_id: u32) -> IconKey {
    IconKey(format!("{WINDOW_ICON_NAMESPACE}:{id}:{process_id}"))
}

/// The window id inside the value of an `hwnd:` key, accepting both the
/// current `<id>:<pid>` form and the older bare `<id>` (the pid is ignored).
pub fn parse_hwnd_icon_value(value: &str) -> Option<u64> {
    let id = value.split_once(':').map_or(value, |(id, _)| id);
    id.trim().parse().ok()
}

/// Whether the window that now has the handle of an `hwnd:` key's `value`
/// is the one the key was made for: its process (`process_id`, from
/// `GetWindowThreadProcessId`; 0 when that failed) is the key's. HWND
/// values are recycled, so a key that outlived its window must not
/// resolve to another process's window. An older key without a
/// pid (`<id>` alone) cannot tell and is taken as it is; a pid that does
/// not parse never matches.
pub fn hwnd_icon_owner_matches(value: &str, process_id: u32) -> bool {
    match value.split_once(':') {
        None => true,
        Some((_, pid)) if pid.trim().is_empty() => true,
        Some((_, pid)) => pid.trim().parse::<u32>().is_ok_and(|pid| pid == process_id),
    }
}

/// Whether `key` names one window's own icon: its namespace, the part
/// before the first `:`, is [`WINDOW_ICON_NAMESPACE`] ([`hwnd_icon_key`]).
/// Such a key is tied to a live window, which may change its icon, and
/// means nothing once that window is gone, so it is never kept beyond the
/// window's life: not as a hidden app's saved icon, not in the
/// platform's in-memory icon cache ([`icons::is_cacheable`]). The
/// one statement of the `hwnd:` rule.
///
/// [`icons::is_cacheable`]: crate::rules::icons::is_cacheable
pub fn is_window_icon_key(key: &IconKey) -> bool {
    key.0
        .split_once(':')
        .is_some_and(|(namespace, _)| namespace == WINDOW_ICON_NAMESPACE)
}
