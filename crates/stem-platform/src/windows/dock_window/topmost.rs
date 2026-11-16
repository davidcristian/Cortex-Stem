//! Keeping a dock window in the topmost band ([`super::MSG_ASSERT_TOPMOST`]).

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GW_HWNDPREV, GetWindow, HWND_TOPMOST, IsWindowVisible, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetWindowPos, WS_EX_TOPMOST,
};

use super::SubclassState;
use super::children::class_name;
use super::styles::get_ex_style;

/// How many windows above the dock the z-order check looks at. The topmost
/// band holds a handful of shell windows; an ordinary window shows up
/// within the first few entries when the dock has dropped out of it.
const MAX_ZORDER_WALK: usize = 64;

/// Put the dock back into the topmost band when an ordinary window
/// has got above it. Reads only in the common case (see the `subclass` docs).
pub(super) fn ensure_topmost(hwnd: HWND, state: &SubclassState, why: &str) {
    let Some(above) = ordinary_window_above(hwnd) else {
        return;
    };
    // SAFETY: the flags change the z-order only; no move, size or
    // activation; failure is reported in the Result.
    let result = unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
    match result {
        Ok(()) => {
            log::debug!(
                "dock window {:#x}: re-asserted HWND_TOPMOST on {why}; it was below {:#x} ({})",
                state.handle,
                above.0 as isize,
                class_name(above)
            );
            // The backdrop window stays where it was in the band.
            crate::windows::backdrop::restack(hwnd);
        }
        Err(e) => log::warn!(
            "dock window {:#x}: SetWindowPos(HWND_TOPMOST) on {why} failed: {e}",
            state.handle
        ),
    }
}

/// The nearest window above `hwnd` in the z-order that is an ordinary app
/// window (visible, not topmost itself, and a taskbar candidate), if there
/// is one within [`MAX_ZORDER_WALK`] entries. A window in the topmost band
/// only has topmost windows above it, so a hit means the band was lost.
///
/// The taskbar-candidate test keeps the shell's z-band windows out: a
/// starting Store app's `Windows.UI.Core.CoreWindow`, the Start menu or
/// the lock screen sit above every desktop-band window without carrying
/// `WS_EX_TOPMOST`, and a re-assert against them would be both useless and
/// a needless reorder among the real topmost windows.
fn ordinary_window_above(hwnd: HWND) -> Option<HWND> {
    let mut current = hwnd;
    for _ in 0..MAX_ZORDER_WALK {
        // SAFETY: GetWindow only reads window-manager state; a handle that
        // is gone makes the call fail.
        let Ok(prev) = (unsafe { GetWindow(current, GW_HWNDPREV) }) else {
            return None;
        };
        if prev.is_invalid() {
            return None;
        }
        // SAFETY: IsWindowVisible accepts any handle value.
        let visible = unsafe { IsWindowVisible(prev) }.as_bool();
        if visible
            && !get_ex_style(prev).is_ok_and(|s| s.contains(WS_EX_TOPMOST))
            && crate::windows::enumerate::is_taskbar_candidate(prev)
        {
            return Some(prev);
        }
        current = prev;
    }
    None
}
