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
//! - [`focus_dock_window`] (the focus shortcut) brings one of our own
//!   dock windows to the foreground through the same strategies as
//!   `Activate` (below), so that it gets the keyboard focus.
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
//!
//! Focusing a dock window. The dock is `WS_EX_NOACTIVATE`: a click
//! never activates it (`dock_window`'s subclass answers `WM_MOUSEACTIVATE`
//! with `MA_NOACTIVATE`), and the system never activates it on its own.
//! The style does not stand in the way of a program that asks: Microsoft's
//! documentation of `WS_EX_NOACTIVATE` names `SetForegroundWindow` and
//! `SetActiveWindow` as the way to activate such a window, and the native
//! context menus already rely on it (muda calls `SetForegroundWindow` on the
//! dock before `TrackPopupMenu`). So [`focus_dock_window`] keeps the
//! style and runs the chain above on the dock's own window: the press of
//! the focus shortcut (`WM_HOTKEY`) makes Stem the process that received
//! the last input event, so step 2 is expected to succeed; a key released
//! in between (the input then went to the app in front) leaves the
//! `AttachThreadInput` step. The activation (`WM_ACTIVATE`, answered by
//! `DefWindowProc` with `SetFocus` on the dock window) reaches the window's
//! own thread, where wry hands the focus on to the WebView2 controller
//! (`MoveFocus` on `WM_SETFOCUS`). The style stays on throughout, so once
//! another window takes the foreground the dock is an ordinary
//! non-activating dock again, with nothing to restore.
//!
//! Elevated windows. The dock runs at medium integrity, so UIPI
//! refuses everything here that posts to a higher-integrity window:
//! `PostMessageW(WM_CLOSE)`, `ShowWindowAsync` (minimise, restore, and the
//! restore step of an activation) fail at once with `ERROR_ACCESS_DENIED`,
//! and `AttachThreadInput` to the elevated thread is refused too.
//! `SetForegroundWindow` itself is not a message and is granted under the
//! usual foreground rules (the elevated Task Manager activates
//! through the plain call). Each refusal is reported, never swallowed:
//! `Minimize`/`Restore`/`Close` return `PlatformError::AccessDenied`
//! ("access denied (elevated or protected process)"), which the command
//! layer logs and shows; `Activate` goes on without the restore (logged
//! once at debug) and reports a failure only when every strategy was
//! refused. All lines here are `debug`: a user action that fails is
//! reported by the command layer once, so nothing is logged twice and
//! nothing repeats while an elevated window merely sits on the dock.

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
use crate::{NativeWindowHandle, PlatformError, PlatformResult};

pub fn perform(id: WindowId, action: WindowAction) -> PlatformResult<()> {
    let hwnd = util::hwnd(id);
    if !is_window(hwnd) {
        return Err(PlatformError::WindowNotFound(id));
    }
    match action {
        WindowAction::Activate => activate(id, hwnd),
        WindowAction::Minimize => show_async(hwnd, SW_MINIMIZE).inspect_err(|e| {
            log::debug!("minimize {id:?}: ShowWindowAsync(SW_MINIMIZE) refused: {e}");
        }),
        WindowAction::Restore => show_async(hwnd, SW_RESTORE).inspect_err(|e| {
            log::debug!("restore {id:?}: ShowWindowAsync(SW_RESTORE) refused: {e}");
        }),
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
/// never waits on the target. Unlike `ShowWindow`'s, its BOOL says whether
/// the request was posted at all: UIPI refuses a post to a window of higher
/// integrity (`ERROR_ACCESS_DENIED` → `PlatformError::AccessDenied`), and
/// that must not pass as success.
fn show_async(hwnd: HWND, cmd: SHOW_WINDOW_CMD) -> PlatformResult<()> {
    // SAFETY: ShowWindowAsync accepts any handle value and a documented command.
    if unsafe { ShowWindowAsync(hwnd, cmd) }.as_bool() {
        return Ok(());
    }
    Err(match util::last_error() {
        // No error code left behind: say what was refused rather than
        // "operation completed successfully".
        PlatformError::Os { code: 0, .. } => {
            PlatformError::Other(format!("ShowWindowAsync({}) refused", cmd.0))
        }
        e => e,
    })
}

fn set_foreground(hwnd: HWND) -> bool {
    // SAFETY: SetForegroundWindow accepts any handle value; it reports refusal
    // through its return value, never by faulting.
    unsafe { SetForegroundWindow(hwnd) }.as_bool()
}

fn activate(id: WindowId, hwnd: HWND) -> PlatformResult<()> {
    // SAFETY: IsIconic accepts any handle value.
    if unsafe { IsIconic(hwnd) }.as_bool()
        && let Err(e) = show_async(hwnd, SW_RESTORE)
    {
        // An elevated window stays minimised (UIPI); the foreground change
        // below is still worth trying, and the failure is reported once.
        log::debug!("activate {id:?}: ShowWindowAsync(SW_RESTORE) refused: {e}; going on");
    }
    if bring_to_foreground(hwnd, &format!("activate {id:?}")) {
        return Ok(());
    }
    Err(PlatformError::Other(format!(
        "could not bring window {} to the foreground",
        id.0
    )))
}

/// Give the keyboard focus to our own dock window `handle`: make it
/// the foreground window through the activation strategies of `Activate`
/// (see the module docs, *Focusing a dock window*). The window must belong
/// to this process; it is never restored or shown here (the app layer
/// shows a hidden dock first). Called off the dock window's thread (the
/// app layer's blocking pool): the activation reaches that thread as a
/// message.
pub fn focus_dock_window(handle: NativeWindowHandle) -> PlatformResult<()> {
    let hwnd = HWND(handle.0 as *mut core::ffi::c_void);
    if !is_window(hwnd) {
        return Err(PlatformError::WindowNotFound(WindowId(handle.0 as u64)));
    }
    if bring_to_foreground(hwnd, &format!("focus dock window {:#x}", handle.0)) {
        return Ok(());
    }
    Err(PlatformError::Other(format!(
        "could not bring dock window {:#x} to the foreground",
        handle.0
    )))
}

/// The strategies of the module docs, first success wins: `true` when
/// `hwnd` is the foreground window afterwards. Each step is logged at debug
/// as `<what>: <strategy>`; a total refusal too, at debug: the caller
/// returns the error, and the command layer reports it once.
fn bring_to_foreground(hwnd: HWND, what: &str) -> bool {
    if is_foreground(hwnd) {
        log::debug!("{what}: already foreground");
        return true;
    }
    if set_foreground(hwnd) {
        log::debug!("{what}: SetForegroundWindow");
        return true;
    }
    log::debug!("{what}: SetForegroundWindow refused; trying AttachThreadInput");
    if activate_with_attached_input(hwnd) {
        log::debug!("{what}: AttachThreadInput");
        return true;
    }
    log::debug!("{what}: AttachThreadInput refused; trying the ALT key trick");
    if activate_with_alt_trick(hwnd) {
        log::debug!("{what}: ALT key trick");
        return true;
    }
    if is_foreground(hwnd) {
        log::debug!("{what}: every call refused but the window is foreground");
        return true;
    }
    log::debug!("{what}: every activation strategy was refused");
    false
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
