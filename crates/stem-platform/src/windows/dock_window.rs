//! OS-level treatment of our own dock windows.
//!
//! Contract:
//! - [`prepare`] runs **once per window, on the window's own thread** (the
//!   Tauri main thread, inside `setup`; the trait doc says so). It ORs the
//!   extended styles the [`DockWindowOptions`] ask for into the window
//!   (`WS_EX_TOOLWINDOW`: hidden from Alt+Tab and the stock taskbar;
//!   `WS_EX_NOACTIVATE`: clicks do not take the foreground), asks DWM not to
//!   round the corners (`DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND`)
//!   when `no_system_decorations`, subclasses the window (`subclass`), lets the
//!   shell's AppBar messages through UIPI (`appbar::allow_shell_messages`,
//!   for an elevated Stem) and applies `WS_EX_NOACTIVATE` to the WebView2
//!   child windows.
//! - [`post_pass_through`] is what the hit-test thread (`hit_test`)
//!   calls on a transition: it posts [`MSG_SET_PASS_THROUGH`], and the
//!   subclass toggles `WS_EX_LAYERED | WS_EX_TRANSPARENT` on the window's
//!   thread. With both bits the window is skipped by mouse hit-testing and
//!   clicks reach whatever is beneath it, in any process (`WM_NCHITTEST →
//!   HTTRANSPARENT` only forwards to windows of the same thread).
//! - [`foreground_handle`] / [`finish_popup`]: the native popup-menu
//!   protocol of the context menus.

mod children;
mod styles;
mod subclass;
mod topmost;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use parking_lot::Mutex;
use stem_core::WindowId;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsWindow, PostMessageW, SetForegroundWindow, SetTimer, WINDOW_EX_STYLE,
    WM_APP, WM_NULL, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

use super::{appbar, util};
use crate::{DockWindowOptions, NativeWindowHandle, PlatformError, PlatformResult};
use children::apply_no_activate_to_children;
use styles::{frame_changed, get_ex_style, set_corner_preference, set_ex_style};
use subclass::subclass_proc;

/// Window message posted to a prepared dock window to make it click-through
/// (`wParam` = 1) or clickable (`wParam` = 0). Handled by the subclass on
/// the window's thread; see [`post_pass_through`].
pub const MSG_SET_PASS_THROUGH: u32 = WM_APP + 0x302;

/// Window message posted to a prepared dock window to check that it is
/// still in the topmost band and put it back if not; see
/// [`assert_topmost_all`].
pub const MSG_ASSERT_TOPMOST: u32 = WM_APP + 0x304;

/// Every dock window [`prepare`] subclassed and `WM_NCDESTROY` has not yet
/// removed: the targets of [`assert_topmost_all`]. Raw handle values only.
static DOCK_WINDOWS: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// Ask every prepared dock window to verify its place in the z-order.
/// Posted, never sent, so it is safe from any thread, including
/// the events thread on every foreground change.
pub fn assert_topmost_all() {
    for &raw in DOCK_WINDOWS.lock().iter() {
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        // SAFETY: posts plain integers to a window of this process; a dead
        // handle only makes the call fail.
        let posted = unsafe { PostMessageW(Some(hwnd), MSG_ASSERT_TOPMOST, WPARAM(0), LPARAM(0)) };
        if let Err(e) = posted {
            log::trace!("PostMessage(MSG_ASSERT_TOPMOST) to {raw:#x} failed: {e}");
        }
    }
}

/// `SetWindowSubclass` id; distinct from wry's and tauri-runtime-wry's
/// subclasses of the same window.
const SUBCLASS_ID: usize = 0x444F_434B; // "DOCK"
/// Window timer that re-applies `WS_EX_NOACTIVATE` to every child once.
const CHILD_STYLE_TIMER_ID: usize = 0x444F_434B;
const CHILD_STYLE_TIMER_MS: u32 = 1500;

/// The two bits that make a window invisible to mouse hit-testing.
const CLICK_THROUGH: WINDOW_EX_STYLE = WINDOW_EX_STYLE(WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0);

/// Reference data of the subclass: what the extended style must look like.
#[derive(Debug)]
struct SubclassState {
    /// The window, for log lines.
    handle: isize,
    /// Extended style bits that must always be present.
    required: AtomicU32,
    /// Whether `WS_EX_LAYERED | WS_EX_TRANSPARENT` are wanted right now.
    pass_through: AtomicBool,
}

impl SubclassState {
    /// `style` corrected to this state: the required bits present and the
    /// click-through bits matching the pass-through flag.
    fn wanted_ex_style(&self, style: WINDOW_EX_STYLE) -> WINDOW_EX_STYLE {
        let required = WINDOW_EX_STYLE(self.required.load(Ordering::Relaxed));
        let with_required = style | required;
        if self.pass_through.load(Ordering::Relaxed) {
            with_required | CLICK_THROUGH
        } else {
            with_required & !CLICK_THROUGH
        }
    }
}

pub fn prepare(handle: NativeWindowHandle, options: DockWindowOptions) -> PlatformResult<()> {
    let hwnd = HWND(handle.0 as *mut core::ffi::c_void);
    // SAFETY: IsWindow accepts any handle value.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(PlatformError::WindowNotFound(WindowId(handle.0 as u64)));
    }

    let mut required = WINDOW_EX_STYLE(0);
    if options.tool_window {
        required |= WS_EX_TOOLWINDOW;
    }
    if options.no_activate {
        required |= WS_EX_NOACTIVATE;
    }

    // The subclass first, so that this function's own style write and every
    // later rewrite by tao go through the same WM_STYLECHANGING correction.
    let state = Box::new(SubclassState {
        handle: handle.0,
        required: AtomicU32::new(required.0),
        pass_through: AtomicBool::new(false),
    });
    let refdata = Box::into_raw(state);
    // SAFETY: called on the thread that owns the window (trait contract);
    // `subclass_proc` is a static function with the SUBCLASSPROC signature;
    // `refdata` is a leaked Box that the procedure frees on WM_NCDESTROY.
    let installed =
        unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, refdata as usize) };
    if !installed.as_bool() {
        // SAFETY: the subclass was not installed, so nothing else holds the
        // pointer; it is freed exactly once, here.
        drop(unsafe { Box::from_raw(refdata) });
        return Err(PlatformError::Other(format!(
            "SetWindowSubclass failed for window {:#x}",
            handle.0
        )));
    }
    // SAFETY: the pointer came from Box::into_raw above and stays valid
    // until WM_NCDESTROY, which cannot be delivered while this function
    // (on the window's own thread) runs.
    let state = unsafe { &*refdata };

    let current = get_ex_style(hwnd)?;
    let next = state.wanted_ex_style(current);
    if next != current {
        set_ex_style(hwnd, next)?;
        frame_changed(hwnd);
    }
    log::debug!(
        "dock window {:#x}: ex-style {:#010x} -> {:#010x}; subclass installed",
        handle.0,
        current.0,
        next.0
    );
    DOCK_WINDOWS.lock().push(handle.0);
    // The shell's AppBar messages reach the subclass even when Stem runs
    // elevated (UIPI).
    appbar::allow_shell_messages(hwnd);

    if options.no_system_decorations {
        set_corner_preference(hwnd)?;
        log::debug!("dock window {:#x}: DWM corner rounding disabled", handle.0);
    }

    apply_no_activate_to_children(hwnd, "at prepare");
    // SAFETY: a timer on a window of this thread, with no callback: the
    // WM_TIMER reaches the subclass, which kills the timer on first delivery.
    let timer = unsafe { SetTimer(Some(hwnd), CHILD_STYLE_TIMER_ID, CHILD_STYLE_TIMER_MS, None) };
    if timer == 0 {
        log::debug!(
            "dock window {:#x}: SetTimer for the late child-style pass failed: {}",
            handle.0,
            util::last_error()
        );
    }
    Ok(())
}

/// Ask a prepared dock window to become click-through (`true`) or clickable
/// (`false`). Asynchronous: the message is posted and applied by the
/// subclass on the window's own thread, so the hit-test thread never waits
/// on the UI thread. `false` when the post failed (dead window, full queue).
pub fn post_pass_through(handle: NativeWindowHandle, pass_through: bool) -> bool {
    let hwnd = HWND(handle.0 as *mut core::ffi::c_void);
    // SAFETY: posts plain integers to a window of this process; a dead
    // handle only makes the call fail.
    let posted = unsafe {
        PostMessageW(
            Some(hwnd),
            MSG_SET_PASS_THROUGH,
            WPARAM(usize::from(pass_through)),
            LPARAM(0),
        )
    };
    match posted {
        Ok(()) => true,
        Err(e) => {
            log::debug!(
                "PostMessage(MSG_SET_PASS_THROUGH) to {:#x} failed: {e}",
                handle.0
            );
            false
        }
    }
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
