//! OS-level treatment of our own dock windows.
//!
//! Contract:
//! - [`prepare`] runs **once per window, on the window's own thread** (the
//!   Tauri main thread, inside `setup`; the trait doc says so). It ORs the
//!   extended styles the [`DockWindowOptions`] ask for into the window
//!   (`WS_EX_TOOLWINDOW`: hidden from Alt+Tab and the stock taskbar;
//!   `WS_EX_NOACTIVATE`: clicks do not take the foreground), asks DWM not to
//!   round the corners (`DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND`)
//!   when `no_system_decorations`, subclasses the window (below) and applies
//!   `WS_EX_NOACTIVATE` to the WebView2 child windows.
//! - [`post_pass_through`] is what the hit-test thread (`hit_test`)
//!   calls on a transition: it posts [`MSG_SET_PASS_THROUGH`], and the
//!   subclass toggles `WS_EX_LAYERED | WS_EX_TRANSPARENT` on the window's
//!   thread. With both bits the window is skipped by mouse hit-testing and
//!   clicks reach whatever is beneath it, in any process (`WM_NCHITTEST →
//!   HTTRANSPARENT` only forwards to windows of the same thread).
//! - [`foreground_handle`] / [`finish_popup`]: the native popup-menu
//!   protocol of the context menus.
//!
//! # Why a subclass
//!
//! tao (Tauri's window layer) keeps `WS_CAPTION | WS_SYSMENU` on undecorated
//! windows and removes the frame by answering `WM_NCCALCSIZE`; disabling DWM
//! non-client rendering (the old `DWMWA_NCRENDERING_POLICY` call) therefore
//! made Windows paint the *legacy* caption whenever activation changed. The
//! call is gone; Tauri's `shadow(false)` already removes the shadow.
//!
//! tao also rewrites `GWL_STYLE` / `GWL_EXSTYLE` from its own flags on every
//! flag change (`show`, `hide`, `set_always_on_top`, …), which discarded the
//! extended styles set here. The subclass (`SetWindowSubclass`, comctl32)
//! makes those rewrites harmless:
//! - `WM_STYLECHANGING` for `GWL_EXSTYLE`: `STYLESTRUCT.styleNew` is
//!   corrected so the required bits are always present and
//!   `WS_EX_LAYERED | WS_EX_TRANSPARENT` match the current pass-through
//!   flag, then the message is forwarded;
//! - `WM_MOUSEACTIVATE` → `MA_NOACTIVATE` (not forwarded);
//! - `WM_PARENTNOTIFY(WM_CREATE)`: `WS_EX_NOACTIVATE` goes onto the new
//!   child window; a one-shot timer [`CHILD_STYLE_TIMER_MS`] after `prepare`
//!   re-applies it to every descendant (WebView2 creates its
//!   `Chrome_WidgetWin_0` tree asynchronously, and some of it may be
//!   created with `WS_EX_NOPARENTNOTIFY`). Chromium's `OnMouseActivate`
//!   answers `MA_NOACTIVATE` for a non-top-level HWND that carries the bit,
//!   so clicking the web content does not activate the dock either;
//! - [`MSG_SET_PASS_THROUGH`]: the flag is updated and the extended style
//!   re-applied (`SetWindowLongPtrW` followed by the `SWP_FRAMECHANGED`
//!   `SetWindowPos` tao itself issues after a style change), then the
//!   z-order is checked as below;
//! - [`MSG_ASSERT_TOPMOST`] (posted by the events thread on every
//!   foreground change through [`assert_topmost_all`]): if an ordinary app
//!   window (visible, not topmost itself, a taskbar candidate) sits above
//!   the dock in the z-order, the dock has dropped out of the topmost band
//!   while keeping its `WS_EX_TOPMOST`
//!   bit (observed on Windows 11 after a lock/unlock cycle with a maximised
//!   browser in front) and `SetWindowPos(HWND_TOPMOST)` puts it back; the
//!   common case is one short bounded `GetWindow(GW_HWNDPREV)` walk and no
//!   write, so re-asserting never reorders the dock among other topmost
//!   windows (menus, the taskbar's flyouts) when it does not have to;
//! - `WM_WINDOWPOSCHANGED` without `SWP_NOZORDER`: logged at trace with the
//!   insert-after handle, so an outside z-order change can be dated;
//! - the AppBar messages (`appbar`): the shell's callback
//!   (`appbar::CALLBACK_MESSAGE`, answered 0), `WM_ACTIVATE`,
//!   `WM_WINDOWPOSCHANGED` and `TaskbarCreated` are handed to `appbar`,
//!   which only queues them for its own thread (this is the main thread,
//!   which must never wait on the shell); all but the callback are
//!   forwarded to the chain as well;
//! - `WM_NCDESTROY`: the window's AppBar is given back (queued), the
//!   subclass is removed and its state freed;
//! - everything else goes to `DefSubclassProc`.
//!
//! The subclass state is a leaked `Box` passed as the reference data; the
//! only cross-thread readers are atomics, and the pointer is freed exactly
//! once, on `WM_NCDESTROY`.
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

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use parking_lot::Mutex;
use stem_core::WindowId;
use windows::Win32::Foundation::{
    GetLastError, HWND, LPARAM, LRESULT, SetLastError, WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GW_HWNDPREV, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetWindow,
    GetWindowLongPtrW, HWND_TOPMOST, IsWindow, IsWindowVisible, KillTimer, MA_NOACTIVATE,
    PostMessageW, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, WINDOW_EX_STYLE,
    WINDOWPOS, WM_ACTIVATE, WM_APP, WM_CREATE, WM_MOUSEACTIVATE, WM_NCDESTROY, WM_NULL,
    WM_PARENTNOTIFY, WM_STYLECHANGING, WM_TIMER, WM_WINDOWPOSCHANGED, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use super::{appbar, util};
use crate::{DockWindowOptions, NativeWindowHandle, PlatformError, PlatformResult};

/// Window message posted to a prepared dock window to make it click-through
/// (`wParam` = 1) or clickable (`wParam` = 0). Handled by the subclass on
/// the window's thread; see [`post_pass_through`].
pub const MSG_SET_PASS_THROUGH: u32 = WM_APP + 0x302;

/// Window message posted to a prepared dock window to check that it is
/// still in the topmost band and put it back if not; see
/// [`assert_topmost_all`].
pub const MSG_ASSERT_TOPMOST: u32 = WM_APP + 0x304;

/// How many windows above the dock the z-order check looks at. The topmost
/// band holds a handful of shell windows; an ordinary window shows up
/// within the first few entries when the dock has dropped out of it.
const MAX_ZORDER_WALK: usize = 64;

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

// ---------------------------------------------------------------------------
// The subclass
// ---------------------------------------------------------------------------

/// `SUBCLASSPROC` installed by [`prepare`]. Runs on the window's thread.
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid: usize,
    refdata: usize,
) -> LRESULT {
    let state_ptr = refdata as *mut SubclassState;
    if msg == WM_NCDESTROY {
        DOCK_WINDOWS.lock().retain(|&raw| raw != hwnd.0 as isize);
        appbar::on_destroyed(hwnd);
        // SAFETY: balances the SetWindowSubclass in `prepare`, on the
        // window's thread; comctl32 keeps the chain valid for this call.
        let removed = unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
        if !removed.as_bool() {
            log::debug!("RemoveWindowSubclass failed on {:#x}", hwnd.0 as isize);
        }
        // SAFETY: forwarding the arguments we were given to the next
        // procedure in the chain.
        let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        if !state_ptr.is_null() {
            // SAFETY: `refdata` is the Box::into_raw pointer from `prepare`;
            // the subclass is removed, so no later message reaches this
            // procedure with it, and it is freed exactly once, here.
            drop(unsafe { Box::from_raw(state_ptr) });
        }
        log::debug!("dock window {:#x}: subclass removed", hwnd.0 as isize);
        return result;
    }

    let handled = if state_ptr.is_null() {
        None
    } else {
        // SAFETY: `refdata` is the live Box from `prepare`; it is only freed
        // on WM_NCDESTROY (above), the last message a window receives.
        let state = unsafe { &*state_ptr };
        on_message(hwnd, msg, wparam, lparam, state)
    };
    match handled {
        Some(result) => result,
        // SAFETY: forwarding the arguments we were given to the next
        // procedure in the chain.
        None => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

/// The subclass's own handling; `None` means "forward to the chain".
fn on_message(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    state: &SubclassState,
) -> Option<LRESULT> {
    match msg {
        // `wParam` is the GWL_* offset, sign-extended to pointer size.
        WM_STYLECHANGING if wparam.0 as isize == GWL_EXSTYLE.0 as isize => {
            let styles = lparam.0 as *mut STYLESTRUCT;
            if styles.is_null() {
                return None;
            }
            // SAFETY: for WM_STYLECHANGING the system passes a STYLESTRUCT
            // that is valid for the duration of this message and is meant
            // to be modified by the window.
            let styles = unsafe { &mut *styles };
            let wanted = state.wanted_ex_style(WINDOW_EX_STYLE(styles.styleNew));
            if wanted.0 != styles.styleNew {
                log::trace!(
                    "dock window {:#x}: ex-style change {:#010x} -> {:#010x} corrected to {:#010x}",
                    state.handle,
                    styles.styleOld,
                    styles.styleNew,
                    wanted.0
                );
                styles.styleNew = wanted.0;
            }
            None
        }
        WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
        WM_PARENTNOTIFY if (wparam.0 & 0xFFFF) as u32 == WM_CREATE => {
            let child = HWND(lparam.0 as *mut core::ffi::c_void);
            let class = class_name(child);
            match apply_no_activate(child) {
                Ok(changed) => log::debug!(
                    "dock window {:#x}: child {:#x} ({class}) created; WS_EX_NOACTIVATE {}",
                    state.handle,
                    child.0 as isize,
                    if changed { "applied" } else { "already set" }
                ),
                Err(e) => log::debug!(
                    "dock window {:#x}: child {:#x} ({class}) created; WS_EX_NOACTIVATE not applied: {e}",
                    state.handle,
                    child.0 as isize
                ),
            }
            None
        }
        WM_TIMER if wparam.0 == CHILD_STYLE_TIMER_ID => {
            // SAFETY: the timer was created on this window by `prepare`.
            if let Err(e) = unsafe { KillTimer(Some(hwnd), CHILD_STYLE_TIMER_ID) } {
                log::debug!("KillTimer(child-style pass) failed: {e}");
            }
            apply_no_activate_to_children(hwnd, "late pass");
            Some(LRESULT(0))
        }
        MSG_SET_PASS_THROUGH => {
            state.pass_through.store(wparam.0 != 0, Ordering::Relaxed);
            apply_pass_through(hwnd, state);
            ensure_topmost(hwnd, state, "pass-through change");
            Some(LRESULT(0))
        }
        MSG_ASSERT_TOPMOST => {
            ensure_topmost(hwnd, state, "foreground change");
            Some(LRESULT(0))
        }
        appbar::CALLBACK_MESSAGE => {
            appbar::on_callback(hwnd, wparam, lparam);
            Some(LRESULT(0))
        }
        WM_ACTIVATE => {
            appbar::on_activate(hwnd, wparam);
            None
        }
        WM_WINDOWPOSCHANGED => {
            appbar::on_window_pos_changed(hwnd);
            let pos = lparam.0 as *const WINDOWPOS;
            if !pos.is_null() {
                // SAFETY: for WM_WINDOWPOSCHANGED the system passes a
                // WINDOWPOS that is valid for the duration of the message.
                let pos = unsafe { &*pos };
                if !pos.flags.contains(SWP_NOZORDER) {
                    log::trace!(
                        "dock window {:#x}: z-order changed from outside; insert after {:#x}, flags {:#010x}",
                        state.handle,
                        pos.hwndInsertAfter.0 as isize,
                        pos.flags.0
                    );
                }
            }
            None
        }
        _ if appbar::is_taskbar_created(msg) => {
            appbar::on_taskbar_created(hwnd);
            None
        }
        _ => None,
    }
}

/// Put the dock back into the topmost band when an ordinary window
/// has got above it. Reads only in the common case (see the module docs).
fn ensure_topmost(hwnd: HWND, state: &SubclassState, why: &str) {
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
        Ok(()) => log::debug!(
            "dock window {:#x}: re-asserted HWND_TOPMOST on {why}; it was below {:#x} ({})",
            state.handle,
            above.0 as isize,
            class_name(above)
        ),
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
            && super::enumerate::is_taskbar_candidate(prev)
        {
            return Some(prev);
        }
        current = prev;
    }
    None
}

/// Re-apply the extended style after the pass-through flag changed: read,
/// correct, write, and refresh the frame as tao does after its own writes.
fn apply_pass_through(hwnd: HWND, state: &SubclassState) {
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

// ---------------------------------------------------------------------------
// WebView2 child windows
// ---------------------------------------------------------------------------

/// OR `WS_EX_NOACTIVATE` into every descendant of `parent` (the WebView2
/// `Chrome_WidgetWin_0` tree); logs the class names and the count at debug.
/// Never fails: a child that refuses is logged and skipped.
fn apply_no_activate_to_children(parent: HWND, when: &str) {
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
fn apply_no_activate(hwnd: HWND) -> PlatformResult<bool> {
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
fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: the buffer is a live, writable slice; the call writes at most
    // its length and returns the number of characters written.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    match usize::try_from(len) {
        Ok(len) if len > 0 => util::from_wide(&buf[..len.min(buf.len())]),
        _ => "?".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Style helpers
// ---------------------------------------------------------------------------

fn get_ex_style(hwnd: HWND) -> PlatformResult<WINDOW_EX_STYLE> {
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

fn set_ex_style(hwnd: HWND, style: WINDOW_EX_STYLE) -> PlatformResult<()> {
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
fn frame_changed(hwnd: HWND) {
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

fn set_corner_preference(hwnd: HWND) -> PlatformResult<()> {
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
