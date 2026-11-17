//! The subclass of a dock window.
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
//! extended styles `prepare` sets. The subclass (`SetWindowSubclass`,
//! comctl32) makes those rewrites harmless:
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
//!   insert-after handle, so an outside z-order change can be dated, and the
//!   dock's backdrop window, if it has one, is restacked under it
//!   ([`crate::windows::backdrop::restack`]; it posts, and so does the
//!   successful re-assert above);
//! - `WM_ACTIVATE`: one debug line per activation change, since
//!   the focus shortcut makes the dock the foreground window
//!   (`actions::focus_dock_window`; the style stays on, nothing here
//!   changes for it); an activation by a click would mean `MA_NOACTIVATE`
//!   was not honoured and says so. Then forwarded like below;
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
//! [`CHILD_STYLE_TIMER_MS`]: super::CHILD_STYLE_TIMER_MS
//! [`assert_topmost_all`]: super::assert_topmost_all

use std::sync::atomic::Ordering;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, KillTimer, MA_NOACTIVATE, STYLESTRUCT, SWP_NOZORDER, WA_CLICKACTIVE, WA_INACTIVE,
    WINDOW_EX_STYLE, WINDOWPOS, WM_ACTIVATE, WM_CREATE, WM_MOUSEACTIVATE, WM_NCDESTROY,
    WM_PARENTNOTIFY, WM_STYLECHANGING, WM_TIMER, WM_WINDOWPOSCHANGED,
};

use super::children::{apply_no_activate, apply_no_activate_to_children, class_name};
use super::styles::apply_pass_through;
use super::topmost::ensure_topmost;
use super::{
    CHILD_STYLE_TIMER_ID, DOCK_WINDOWS, MSG_ASSERT_TOPMOST, MSG_SET_PASS_THROUGH, SUBCLASS_ID,
    SubclassState,
};
use crate::windows::appbar;

/// `SUBCLASSPROC` installed by [`prepare`](super::prepare). Runs on the window's thread.
pub(super) unsafe extern "system" fn subclass_proc(
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
            log_activation(state, wparam, lparam);
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
                    crate::windows::backdrop::restack(hwnd);
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

/// Say in the log when the dock window became the foreground window
/// and when it stopped being it. The low word of
/// `wParam` is `WA_INACTIVE`, `WA_ACTIVE` or `WA_CLICKACTIVE`; `lParam` is
/// the window on the other side of the change (the one activated when the
/// dock is deactivated, the one deactivated otherwise), possibly NULL.
fn log_activation(state: &SubclassState, wparam: WPARAM, lparam: LPARAM) {
    let how = (wparam.0 & 0xFFFF) as u32;
    let other = lparam.0;
    if how == WA_INACTIVE {
        log::debug!(
            "dock window {:#x}: deactivated; {other:#x} is activated",
            state.handle
        );
    } else if how == WA_CLICKACTIVE {
        // `WM_MOUSEACTIVATE` answers `MA_NOACTIVATE`, so this is unexpected.
        log::debug!(
            "dock window {:#x}: activated by a click (unexpected for a no-activate window); {other:#x} is deactivated",
            state.handle
        );
    } else {
        log::debug!(
            "dock window {:#x}: activated; {other:#x} is deactivated",
            state.handle
        );
    }
}
