//! Extended-style and DWM helpers for the dock windows and their children.
//!
//! Implementation notes:
//! - `GetWindowLongPtrW`/`SetWindowLongPtrW` report failure with a return
//!   value of 0, which is also a valid style, so `SetLastError(0)` is called
//!   first and `GetLastError` decides.
//! - Style changes are cached by the window manager until a
//!   `SetWindowPos(SWP_FRAMECHANGED)`; one is issued (no move, size, z-order
//!   or activation change) so the new styles apply even if the window was
//!   already shown.
//! - `SetWindowLongPtrW(GWL_EXSTYLE)` on the WebView2 children works across
//!   processes at the same integrity level; a refusal is logged, never
//!   fatal.

use std::sync::atomic::Ordering;

use windows::Win32::Foundation::{GetLastError, HWND, SetLastError, WIN32_ERROR};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WINDOW_EX_STYLE,
};

use super::SubclassState;
use crate::PlatformResult;

/// Re-apply the extended style after the pass-through flag changed: read,
/// correct, write, and refresh the frame as tao does after its own writes.
pub(super) fn apply_pass_through(hwnd: HWND, state: &SubclassState) {
    let current = match get_ex_style(hwnd) {
        Ok(style) => style,
        Err(e) => {
            log::debug!(
                "dock window {:#x}: cannot read the ex-style: {e}",
                state.handle
            );
            return;
        }
    };
    let wanted = state.wanted_ex_style(current);
    if wanted == current {
        return;
    }
    if let Err(e) = set_ex_style(hwnd, wanted) {
        log::warn!(
            "dock window {:#x}: cannot set the ex-style: {e}",
            state.handle
        );
        return;
    }
    frame_changed(hwnd);
    log::debug!(
        "dock window {:#x}: ex-style {:#010x} -> {:#010x} ({})",
        state.handle,
        current.0,
        wanted.0,
        if state.pass_through.load(Ordering::Relaxed) {
            "click-through"
        } else {
            "clickable"
        }
    );
}

pub(super) fn get_ex_style(hwnd: HWND) -> PlatformResult<WINDOW_EX_STYLE> {
    // SAFETY: SetLastError has no preconditions.
    unsafe { SetLastError(WIN32_ERROR(0)) };
    // SAFETY: GetWindowLongPtrW reads a window property; a dead handle
    // makes the call fail, which GetLastError reports.
    let value = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    if value == 0 {
        // SAFETY: GetLastError has no preconditions.
        let err = unsafe { GetLastError() };
        if err.0 != 0 {
            return Err(windows::core::Error::from_thread().into());
        }
    }
    Ok(WINDOW_EX_STYLE(value as u32))
}

pub(super) fn set_ex_style(hwnd: HWND, style: WINDOW_EX_STYLE) -> PlatformResult<()> {
    // SAFETY: SetLastError has no preconditions.
    unsafe { SetLastError(WIN32_ERROR(0)) };
    // LONG_PTR takes the sign-extended LONG, as the C macros do.
    let value = (style.0 as i32) as isize;
    // SAFETY: SetWindowLongPtrW sets a window property; the value is a plain
    // style bit set; a refusal (dead handle, other integrity level) is
    // reported through GetLastError.
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, value) };
    if previous == 0 {
        // SAFETY: GetLastError has no preconditions.
        let err = unsafe { GetLastError() };
        if err.0 != 0 {
            return Err(windows::core::Error::from_thread().into());
        }
    }
    Ok(())
}

/// Make the window manager pick up a style change: no move, size, z-order
/// or activation change. Failure is logged; the styles are set regardless.
pub(super) fn frame_changed(hwnd: HWND) {
    // SAFETY: the flags request a frame refresh only.
    let result = unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
    };
    if let Err(e) = result {
        log::debug!(
            "SetWindowPos(SWP_FRAMECHANGED) on {:#x} failed: {e}",
            hwnd.0 as isize
        );
    }
}

pub(super) fn set_corner_preference(hwnd: HWND) -> PlatformResult<()> {
    let raw: i32 = DWMWCP_DONOTROUND.0;
    let ptr = std::ptr::from_ref(&raw).cast::<core::ffi::c_void>();
    // SAFETY: `ptr` addresses a live i32 and the size passed is exactly that
    // of an i32, the documented payload of DWMWA_WINDOW_CORNER_PREFERENCE.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            ptr,
            size_of::<i32>() as u32,
        )
    }?;
    Ok(())
}
