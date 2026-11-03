//! OS-level treatment of our own dock windows.
//!
//! Contract:
//! - [`prepare`] adjusts the extended style of the given `HWND`:
//!   `WS_EX_TOOLWINDOW` (hidden from Alt+Tab and the stock taskbar) and
//!   `WS_EX_NOACTIVATE` (clicks do not steal focus) when requested; sets
//!   `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND` and disables the
//!   DWM shadow (`DWMWA_NCRENDERING_POLICY`) when `no_system_decorations`.
//!   Must be called after Tauri created the window and before it is shown.
//! - [`set_hit_region`] subclasses the window (`SetWindowSubclass`) so that
//!   `WM_NCHITTEST` returns `HTTRANSPARENT` outside `region`; `None` removes
//!   the subclass. (M3)
//!
//! Implementation notes:
//! - `GetWindowLongPtrW`/`SetWindowLongPtrW` report failure with a return
//!   value of 0, which is also a valid style, so `SetLastError(0)` is called
//!   first and `GetLastError` decides.
//! - Style changes are cached by the window manager until a
//!   `SetWindowPos(SWP_FRAMECHANGED)`; one is issued (no move, size, z-order
//!   or activation change) so the new styles apply even if the window was
//!   already shown.

use stem_core::{Rect, WindowId};
use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, SetLastError, WIN32_ERROR, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED, DWMWA_NCRENDERING_POLICY,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, IsWindow, PostMessageW, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, WINDOW_EX_STYLE, WM_NULL, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::{DockWindowOptions, NativeWindowHandle, PlatformError, PlatformResult};

pub fn prepare(handle: NativeWindowHandle, options: DockWindowOptions) -> PlatformResult<()> {
    let hwnd = HWND(handle.0 as *mut core::ffi::c_void);
    // SAFETY: IsWindow accepts any handle value.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(PlatformError::WindowNotFound(WindowId(handle.0 as u64)));
    }

    let mut wanted = WINDOW_EX_STYLE(0);
    if options.tool_window {
        wanted |= WS_EX_TOOLWINDOW;
    }
    if options.no_activate {
        wanted |= WS_EX_NOACTIVATE;
    }
    if wanted.0 != 0 {
        let current = get_ex_style(hwnd)?;
        let next = current | wanted;
        if next != current {
            set_ex_style(hwnd, next)?;
            // SAFETY: the flags request a frame refresh only; no position,
            // size, z-order or activation change.
            unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                )
            }?;
        }
        log::debug!(
            "dock window {:#x}: ex-style {:#010x} -> {:#010x}",
            handle.0,
            current.0,
            next.0
        );
    }

    if options.no_system_decorations {
        set_dwm_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND)?;
        set_dwm_attribute(hwnd, DWMWA_NCRENDERING_POLICY, DWMNCRP_DISABLED)?;
        log::debug!(
            "dock window {:#x}: DWM rounding and NC rendering disabled",
            handle.0
        );
    }
    Ok(())
}

pub fn set_hit_region(_handle: NativeWindowHandle, _region: Option<Rect>) -> PlatformResult<()> {
    Err(PlatformError::Unsupported("click-through"))
}

/// The window that has the foreground right now, if any.
pub fn foreground_handle() -> Option<NativeWindowHandle> {
    // SAFETY: GetForegroundWindow has no preconditions; it returns a null
    // handle when no window is in the foreground.
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.0.is_null()).then_some(NativeWindowHandle(hwnd.0 as isize))
}

/// After `TrackPopupMenu` returned on `dock`: post the `WM_NULL` of
/// the documented recipe (KB 135788) so the next click outside a menu is
/// delivered as such, then hand the foreground back to `previous`. The dock
/// is `WS_EX_NOACTIVATE` and must never stay focused; the context-menu
/// library gave it the foreground so the menu could be dismissed.
pub fn finish_popup(dock: NativeWindowHandle, previous: Option<NativeWindowHandle>) {
    let dock = HWND(dock.0 as *mut core::ffi::c_void);
    // SAFETY: posts a no-op message to a window of this process; a dead
    // handle only makes the call fail.
    if let Err(e) = unsafe { PostMessageW(Some(dock), WM_NULL, WPARAM(0), LPARAM(0)) } {
        log::debug!("PostMessage(WM_NULL) after the context menu failed: {e}");
    }
    let Some(previous) = previous else {
        return;
    };
    let previous = HWND(previous.0 as *mut core::ffi::c_void);
    if previous == dock {
        return;
    }
    // SAFETY: IsWindow accepts any handle value.
    if !unsafe { IsWindow(Some(previous)) }.as_bool() {
        return;
    }
    // SAFETY: SetForegroundWindow has no memory preconditions; it merely
    // fails when this process no longer holds the foreground right.
    if !unsafe { SetForegroundWindow(previous) }.as_bool() {
        log::debug!(
            "could not hand the foreground back to {:#x} after the context menu",
            previous.0 as isize
        );
    }
}

fn get_ex_style(hwnd: HWND) -> PlatformResult<WINDOW_EX_STYLE> {
    // SAFETY: SetLastError has no preconditions.
    unsafe { SetLastError(WIN32_ERROR(0)) };
    // SAFETY: GetWindowLongPtrW reads a window property of a live window.
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

fn set_ex_style(hwnd: HWND, style: WINDOW_EX_STYLE) -> PlatformResult<()> {
    // SAFETY: SetLastError has no preconditions.
    unsafe { SetLastError(WIN32_ERROR(0)) };
    // LONG_PTR takes the sign-extended LONG, as the C macros do.
    let value = (style.0 as i32) as isize;
    // SAFETY: SetWindowLongPtrW sets a window property of a live window owned
    // by this process; the value is a plain style bit set.
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

trait DwmValue: Copy {
    fn raw(self) -> i32;
}

impl DwmValue for DWM_WINDOW_CORNER_PREFERENCE {
    fn raw(self) -> i32 {
        self.0
    }
}

impl DwmValue for DWMNCRENDERINGPOLICY {
    fn raw(self) -> i32 {
        self.0
    }
}

fn set_dwm_attribute<V: DwmValue>(
    hwnd: HWND,
    attribute: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE,
    value: V,
) -> PlatformResult<()> {
    let raw: i32 = value.raw();
    let ptr = std::ptr::from_ref(&raw).cast::<core::ffi::c_void>();
    // SAFETY: `ptr` addresses a live i32 and the size passed is exactly that
    // of an i32, which is the documented payload for both attributes used here.
    unsafe { DwmSetWindowAttribute(hwnd, attribute, ptr, size_of::<i32>() as u32) }?;
    Ok(())
}
