//! The WebView2 child windows of a dock window, which get `WS_EX_NOACTIVATE`
//! too.

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetClassNameW, WS_EX_NOACTIVATE};
use windows::core::BOOL;

use super::styles::{get_ex_style, set_ex_style};
use crate::PlatformResult;
use crate::windows::util;

/// OR `WS_EX_NOACTIVATE` into every descendant of `parent` (the WebView2
/// `Chrome_WidgetWin_0` tree); logs the class names and the count at debug.
/// Never fails: a child that refuses is logged and skipped.
pub(super) fn apply_no_activate_to_children(parent: HWND, when: &str) {
    let children = child_windows(parent);
    let mut applied = 0usize;
    let mut classes = Vec::with_capacity(children.len());
    for &child in &children {
        let class = class_name(child);
        match apply_no_activate(child) {
            Ok(changed) => {
                if changed {
                    applied += 1;
                }
                classes.push(class);
            }
            Err(e) => log::debug!(
                "dock window child {:#x} ({class}): WS_EX_NOACTIVATE not applied: {e}",
                child.0 as isize
            ),
        }
    }
    log::debug!(
        "dock window {:#x}: WS_EX_NOACTIVATE on {} child window(s), {applied} newly set ({when}): {classes:?}",
        parent.0 as isize,
        children.len()
    );
}

/// OR `WS_EX_NOACTIVATE` into one window's extended style. `Ok(true)` when
/// the bit was added, `Ok(false)` when it was already there.
pub(super) fn apply_no_activate(hwnd: HWND) -> PlatformResult<bool> {
    let current = get_ex_style(hwnd)?;
    if current.contains(WS_EX_NOACTIVATE) {
        return Ok(false);
    }
    set_ex_style(hwnd, current | WS_EX_NOACTIVATE)?;
    Ok(true)
}

/// Every descendant of `parent` (`EnumChildWindows` recurses).
fn child_windows(parent: HWND) -> Vec<HWND> {
    let mut list: Vec<HWND> = Vec::new();
    let lparam = LPARAM(std::ptr::from_mut(&mut list) as isize);
    // SAFETY: `collect_child` only pushes into the Vec whose address travels
    // in lParam; the Vec outlives the call, which runs on this thread.
    let _ = unsafe { EnumChildWindows(Some(parent), Some(collect_child), lparam) };
    list
}

unsafe extern "system" fn collect_child(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = lparam.0 as *mut Vec<HWND>;
    // SAFETY: `lparam` is the pointer `child_windows` passed to
    // EnumChildWindows; the Vec is alive and not otherwise borrowed while
    // the enumeration runs on this thread.
    let list = unsafe { &mut *list };
    list.push(hwnd);
    BOOL(1)
}

/// The window's class name (capped at 256 characters by the API), `"?"`
/// when it cannot be read.
pub(super) fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: the buffer is a live, writable slice; the call writes at most
    // its length and returns the number of characters written.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    match usize::try_from(len) {
        Ok(len) if len > 0 => util::from_wide(&buf[..len.min(buf.len())]),
        _ => "?".to_owned(),
    }
}
