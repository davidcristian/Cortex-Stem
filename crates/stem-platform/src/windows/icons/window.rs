//! The `hwnd:` chain: a window's own icon, its class icons, and its exe's
//! icon when that is larger.

use stem_core::WindowId;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GCLP_HICON, GCLP_HICONSM, GET_CLASS_LONG_INDEX, GetClassLongPtrW, GetWindowThreadProcessId,
    HICON, ICON_BIG, ICON_SMALL, IsWindow, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETICON,
};

use super::bitmap::{icon_dimensions, icon_to_image};
use super::{Loaded, load_file_icon};
use crate::PlatformResult;
use crate::rules::identity::{hwnd_icon_owner_matches, parse_hwnd_icon_value};
use crate::windows::util;

/// Cross-process `WM_GETICON` budget (`docs/development/windows-rules.md`: ≤ 200 ms).
const GETICON_TIMEOUT_MS: u32 = 200;

/// `hwnd:` chain (see the `icons` docs). `value` is `<id>:<pid>` or the older
/// bare `<id>`.
pub(super) fn load_window_icon(value: &str, size: u32) -> PlatformResult<Option<Loaded>> {
    let Some(id) = parse_hwnd_icon_value(value) else {
        return Ok(None);
    };
    let hwnd = util::hwnd(WindowId(id));
    // SAFETY: IsWindow accepts any handle value and only reads.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        log::debug!("hwnd:{id}: window is gone");
        return Ok(None);
    }
    // HWND values are recycled: a key that outlived its window may name
    // another process's window now.
    let mut process_id = 0u32;
    // SAFETY: `process_id` is a live out-pointer for the duration of the call;
    // a handle that went away meanwhile makes it return 0 and leaves it 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if !hwnd_icon_owner_matches(value, process_id) {
        log::debug!(
            "hwnd:{value}: the handle belongs to another process ({process_id}) now; no icon"
        );
        return Ok(None);
    }
    let window =
        largest_window_icon(hwnd).and_then(|candidate| match icon_to_image(candidate.icon) {
            Ok(Some(image)) => Some(Loaded::new(candidate.source, image)),
            Ok(None) => None,
            Err(e) => {
                log::debug!("hwnd:{id}: converting the window icon failed: {e}");
                None
            }
        });
    // A window icon at least as large as requested is the answer.
    if let Some(found) = &window
        && found.image.edge() >= size
    {
        return Ok(window);
    }
    // Otherwise the exe's icon (large, high quality) is preferred, when the
    // process is known and it really is larger than what the window offers.
    if let Some(path) = window_exe_path(hwnd, id) {
        match load_file_icon(&path, size) {
            Ok(Some(from_exe))
                if window
                    .as_ref()
                    .is_none_or(|w| from_exe.image.edge() > w.image.edge()) =>
            {
                return Ok(Some(from_exe));
            }
            Ok(Some(from_exe)) => log::trace!(
                "hwnd:{id}: exe icon ({}x{}) is no larger than the window icon; keeping the window icon",
                from_exe.image.width,
                from_exe.image.height
            ),
            Ok(None) => {}
            Err(e) => log::debug!("hwnd:{id}: exe icon failed: {e}"),
        }
    }
    Ok(window)
}

/// A shared window or class icon with its source and measured edge.
#[derive(Debug, Clone, Copy)]
struct WindowIconCandidate {
    icon: HICON,
    source: &'static str,
    edge: u32,
}

/// Gather the window's icon candidates and pick the largest by
/// `GetIconInfo` dimensions (ties keep the earlier one: `ICON_BIG` first).
/// Shared handles; nothing here is destroyed. `None` when the window offers
/// no icon at all.
fn largest_window_icon(hwnd: HWND) -> Option<WindowIconCandidate> {
    let mut candidates: Vec<(HICON, &'static str)> = Vec::with_capacity(4);
    let mut push = |icon: Option<HICON>, source: &'static str| {
        if let Some(icon) = icon
            && !candidates.iter().any(|(seen, _)| seen.0 == icon.0)
        {
            candidates.push((icon, source));
        }
    };
    match window_message_icon(hwnd, ICON_BIG) {
        Ok(big) => {
            push(big, "WM_GETICON(ICON_BIG)");
            push(
                window_message_icon(hwnd, ICON_SMALL).ok().flatten(),
                "WM_GETICON(ICON_SMALL)",
            );
        }
        Err(e) => log::trace!(
            "window {:?}: WM_GETICON(ICON_BIG) got no answer ({e}); skipping ICON_SMALL",
            hwnd.0
        ),
    }
    push(class_icon(hwnd, GCLP_HICON), "GCLP_HICON");
    push(class_icon(hwnd, GCLP_HICONSM), "GCLP_HICONSM");

    let mut best: Option<WindowIconCandidate> = None;
    for (icon, source) in candidates {
        let Some((w, h)) = icon_dimensions(icon) else {
            continue;
        };
        let edge = w.max(h);
        if best.is_none_or(|b| edge > b.edge) {
            best = Some(WindowIconCandidate { icon, source, edge });
        }
    }
    best
}

/// `WM_GETICON` with a bounded timeout. `Ok` carries the window's answer
/// (an icon that belongs to the window, or none); `Err` means the window
/// did not answer at all, and says why: `AccessDenied` for a window of
/// higher integrity (UIPI refuses the message at once), `Os` with
/// `ERROR_TIMEOUT` for a hung one. Further messages are then pointless.
fn window_message_icon(hwnd: HWND, which: u32) -> PlatformResult<Option<HICON>> {
    let mut result: usize = 0;
    // SAFETY: WM_GETICON carries no pointers; `result` is a valid out-pointer;
    // a hung or elevated target makes the call return 0 within the timeout.
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            WPARAM(which as usize),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            GETICON_TIMEOUT_MS,
            Some(&mut result),
        )
    };
    if sent.0 == 0 {
        return Err(util::last_error());
    }
    if result == 0 {
        return Ok(None);
    }
    Ok(Some(HICON(result as *mut core::ffi::c_void)))
}

/// The window class icon. Shared; not destroyed.
fn class_icon(hwnd: HWND, index: GET_CLASS_LONG_INDEX) -> Option<HICON> {
    // SAFETY: GetClassLongPtrW only reads class data; it works across
    // processes and integrity levels.
    let value = unsafe { GetClassLongPtrW(hwnd, index) };
    (value != 0).then_some(HICON(value as *mut core::ffi::c_void))
}

/// The window's process image path (as a string), for the exe fallback of
/// the `hwnd:` chain. `None` when the window no longer qualifies or the
/// process cannot be read.
fn window_exe_path(hwnd: HWND, id: u64) -> Option<String> {
    match crate::windows::enumerate::query_window(hwnd) {
        Ok(Some(raw)) => raw.exe_path.map(|p| p.to_string_lossy().into_owned()),
        Ok(None) => None,
        Err(e) => {
            log::debug!("hwnd:{id}: no exe icon fallback (query_window: {e})");
            None
        }
    }
}
