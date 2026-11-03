//! Window actions.
//!
//! Contract:
//! - [`perform`] implements `Activate` (restore if minimised, then bring to
//!   foreground honouring the `SetForegroundWindow` rules with the documented
//!   fallbacks: `AllowSetForegroundWindow`, the ALT key-press trick,
//!   `AttachThreadInput`; log which path was used), `Minimize`
//!   (`ShowWindowAsync(SW_MINIMIZE)`), `Restore` (`SW_RESTORE`), and `Close`
//!   (`PostMessageW(WM_CLOSE)` — never `DestroyWindow`).
//! - [`foreground_window`] returns the current foreground window if it is a
//!   taskbar candidate (see `enumerate::is_taskbar_candidate`).
//! - Returns `WindowNotFound` for dead handles, never panics.
//!
//! Activation strategy (first success wins; the winning step is logged at
//! debug, the fallbacks taken at debug too so a log can tell them apart):
//! 1. `ShowWindowAsync(SW_RESTORE)` when the window is minimised (posted,
//!    never waits on the target), then plain `SetForegroundWindow`. The dock
//!    received the last input event (the click), which is one of the
//!    conditions under which the system grants the call even from a
//!    `WS_EX_NOACTIVATE` window, so this often succeeds.
//! 2. `AttachThreadInput` to the current foreground thread, then
//!    `SetForegroundWindow` + `BringWindowToTop`, then detach: sharing the
//!    foreground thread's input queue satisfies the foreground-lock rules
//!    without synthesising any input.
//! 3. The ALT trick, last because it touches the keyboard state: a synthetic
//!    ALT *down* makes the system consider our process the last one to
//!    receive input, `SetForegroundWindow` is called while the key is held,
//!    and ALT is released afterwards. The previously active window sees only
//!    the press and the newly activated one only the release, so neither
//!    sees a bare press/release pair, which would put its menu bar into
//!    keyboard-menu mode.
//! 4. If the window is the foreground window afterwards anyway, report
//!    success; otherwise fail with `PlatformError::Other`.
//!
//! `AllowSetForegroundWindow` only grants the right to *another* process and
//! does not help a caller that lacks it, so it is not part of the chain.
//! Nothing here sends a synchronous message to the target process.

use stem_core::{WindowAction, WindowId};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_MENU, keybd_event,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow,
    PostMessageW, SHOW_WINDOW_CMD, SW_MINIMIZE, SW_RESTORE, SetForegroundWindow, ShowWindowAsync,
    WM_CLOSE,
};

use super::{enumerate, util};
use crate::{PlatformError, PlatformResult};

pub fn perform(id: WindowId, action: WindowAction) -> PlatformResult<()> {
    let hwnd = util::hwnd(id);
    if !is_window(hwnd) {
        return Err(PlatformError::WindowNotFound(id));
    }
    match action {
        WindowAction::Activate => activate(id, hwnd),
        WindowAction::Minimize => {
            show_async(hwnd, SW_MINIMIZE);
            Ok(())
        }
        WindowAction::Restore => {
            show_async(hwnd, SW_RESTORE);
            Ok(())
        }
        WindowAction::Close => {
            // SAFETY: PostMessageW queues the message and returns immediately;
            // the parameters are plain integers.
            match unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
                Ok(()) => Ok(()),
                Err(_) if !is_window(hwnd) => Err(PlatformError::WindowNotFound(id)),
                Err(e) => Err(e.into()),
            }
        }
    }
}

pub fn foreground_window() -> Option<WindowId> {
    // SAFETY: GetForegroundWindow has no preconditions; NULL is handled.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    enumerate::is_taskbar_candidate(hwnd).then(|| util::window_id(hwnd))
}

fn is_window(hwnd: HWND) -> bool {
    // SAFETY: IsWindow accepts any handle value.
    unsafe { IsWindow(Some(hwnd)) }.as_bool()
}

fn is_foreground(hwnd: HWND) -> bool {
    // SAFETY: GetForegroundWindow has no preconditions.
    unsafe { GetForegroundWindow() == hwnd }
}

/// `ShowWindowAsync`: the request is posted to the owning thread, so this
/// never waits on the target. The BOOL only reports the previous visibility.
fn show_async(hwnd: HWND, cmd: SHOW_WINDOW_CMD) {
    // SAFETY: ShowWindowAsync accepts any handle value and a documented command.
    let _previously_visible = unsafe { ShowWindowAsync(hwnd, cmd) };
}

fn set_foreground(hwnd: HWND) -> bool {
    // SAFETY: SetForegroundWindow accepts any handle value; it reports refusal
    // through its return value, never by faulting.
    unsafe { SetForegroundWindow(hwnd) }.as_bool()
}

fn activate(id: WindowId, hwnd: HWND) -> PlatformResult<()> {
    // SAFETY: IsIconic accepts any handle value.
    if unsafe { IsIconic(hwnd) }.as_bool() {
        show_async(hwnd, SW_RESTORE);
    }
    if is_foreground(hwnd) {
        log::debug!("activate {id:?}: already foreground");
        return Ok(());
    }

    if set_foreground(hwnd) {
        log::debug!("activate {id:?}: SetForegroundWindow");
        return Ok(());
    }
    log::debug!("activate {id:?}: SetForegroundWindow refused; trying AttachThreadInput");
    if activate_with_attached_input(hwnd) {
        log::debug!("activate {id:?}: AttachThreadInput");
        return Ok(());
    }
    log::debug!("activate {id:?}: AttachThreadInput refused; trying the ALT key trick");
    if activate_with_alt_trick(hwnd) {
        log::debug!("activate {id:?}: ALT key trick");
        return Ok(());
    }
    if is_foreground(hwnd) {
        log::debug!("activate {id:?}: every call refused but the window is foreground");
        return Ok(());
    }
    log::warn!("activate {id:?}: every activation strategy was refused");
    Err(PlatformError::Other(format!(
        "could not bring window {} to the foreground",
        id.0
    )))
}

/// Hold ALT down (extended key, no scan code) across the call and release
/// it afterwards, so no window sees a bare press/release pair.
fn activate_with_alt_trick(hwnd: HWND) -> bool {
    let alt = VK_MENU.0 as u8;
    // SAFETY: keybd_event synthesises input for our own thread's queue; the
    // flags are documented values.
    unsafe { keybd_event(alt, 0, KEYEVENTF_EXTENDEDKEY, 0) };
    let ok = set_foreground(hwnd);
    // SAFETY: as above; this releases the key pressed just before, after the
    // foreground change so the new window never sees the press.
    unsafe { keybd_event(alt, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0) };
    ok
}

/// Temporarily share the input queue of the current foreground thread.
fn activate_with_attached_input(hwnd: HWND) -> bool {
    // SAFETY: GetForegroundWindow has no preconditions.
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_invalid() {
        return false;
    }
    // SAFETY: a NULL out-pointer is allowed when only the thread id is wanted.
    let foreground_thread = unsafe { GetWindowThreadProcessId(foreground, None) };
    // SAFETY: GetCurrentThreadId has no preconditions.
    let own_thread = unsafe { GetCurrentThreadId() };
    if foreground_thread == 0 || foreground_thread == own_thread {
        return false;
    }
    // SAFETY: both ids are live threads; a failure is reported in the BOOL.
    if !unsafe { AttachThreadInput(own_thread, foreground_thread, true) }.as_bool() {
        log::debug!("AttachThreadInput({own_thread} -> {foreground_thread}) refused");
        return false;
    }
    let ok = set_foreground(hwnd);
    // SAFETY: BringWindowToTop accepts any handle value; failure is non-fatal.
    if let Err(e) = unsafe { BringWindowToTop(hwnd) } {
        log::debug!("BringWindowToTop failed: {e}");
    }
    // SAFETY: balances the attach above.
    let _detached = unsafe { AttachThreadInput(own_thread, foreground_thread, false) };
    ok
}
