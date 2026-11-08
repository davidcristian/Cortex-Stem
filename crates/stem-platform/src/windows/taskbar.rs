//! The stock taskbar's auto-hide state.
//!
//! Contract:
//! - [`auto_hide`] -> `SHAppBarMessage(ABM_GETSTATE)`: whether Windows' own
//!   *Automatically hide the taskbar* is on (`ABS_AUTOHIDE` in the returned
//!   state bits). `ABS_ALWAYSONTOP` is not reported since Windows 7 (the
//!   taskbar is always on top) and is ignored here. Fails when there is no
//!   tray to ask (below) and when Explorer does not answer in time.
//! - [`set_auto_hide`] -> `SHAppBarMessage(ABM_SETSTATE)` with `lParam` =
//!   `ABS_AUTOHIDE` or `0` (the only bit `ABM_SETSTATE` still honours), then
//!   `ABM_GETSTATE` again: `ABM_SETSTATE` always "succeeds", so the read-back
//!   is the only way to know that the tray took the state.
//!
//! Windows persists the state itself: the Taskbar settings page follows
//! (*Taskbar behaviours* → *Automatically hide the taskbar*) and it survives
//! an Explorer restart. Nothing is written to the registry here. Which state
//! to set when, and the restore of the user's own, are `rules::taskbar` and
//! the app layer; the marker file lives there too.
//!
//! # No tray
//!
//! With no tray window at all (Explorer crashed, is restarting, or is not
//! running) `ABM_GETSTATE` answers `0`, which reads exactly like "off", and
//! `ABM_SETSTATE` changes nothing. So every message is preceded by
//! `FindWindowW("Shell_TrayWnd")`, and an `ABM_GETSTATE` answer counts only
//! when the same tray window still exists after it (`IsWindow`): otherwise
//! the call fails, and `rules::taskbar` neither writes nor deletes the
//! marker on a state it could not read. `FindWindowW` and `IsWindow` read
//! win32k's window list and send nothing.
//!
//! # Bounded waits
//!
//! `SHAppBarMessage` is a `SendMessageW` to `Shell_TrayWnd` with no timeout
//! of its own: a hung Explorer would hold the calling thread for as long as
//! it hangs. Each platform call therefore runs its messages on a
//! short-lived helper thread (`stem-taskbar`; [`set_auto_hide`]'s set and
//! its read-back share one) and waits for the answer on a channel for at
//! most [`SHELL_TIMEOUT`] (500 ms); past that it returns
//! `PlatformError::Timeout` and abandons the helper, which ends by itself
//! when Explorer answers (or dies), or with the process. While an abandoned
//! helper has not answered, every further call fails at once with
//! `Timeout` instead of starting another one, so at most one thread is ever
//! left waiting on Explorer and a hung Explorer costs one 500 ms wait,
//! not one per call. The calls themselves are serialised.
//!
//! Worst cases: one call returns within about 500 ms; the app's exit
//! restore (one [`auto_hide`], then at most one [`set_auto_hide`], on the
//! main thread at `RunEvent::Exit`) within about 1 s, and within about
//! 500 ms when Explorer is hung outright (the second call fails at once).
//! No window of ours is involved (`hWnd` stays null), so nothing calls back
//! into the dock window's thread. A set that the tray takes but answers
//! only after the timeout reports `Timeout` although the state changed;
//! the read-back of the next call sees it.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, TryRecvError};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Shell::{
    ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindow};
use windows::core::{PCWSTR, w};

use crate::{PlatformError, PlatformResult};

/// How long a platform call waits for Explorer's answer.
pub const SHELL_TIMEOUT: Duration = Duration::from_millis(500);

/// What a helper thread answers: the `ABS_*` state bits it read.
type Answer = PlatformResult<u32>;

/// The answer channel of the helper that timed out and has not answered
/// yet, if any. Held for the whole of a call, which serialises the calls.
static ABANDONED: Mutex<Option<Receiver<Answer>>> = Mutex::new(None);

/// Whether the taskbar auto-hides right now. Fails without a tray
/// (`PlatformError::Other`) and when Explorer does not answer within
/// [`SHELL_TIMEOUT`] (`PlatformError::Timeout`).
pub fn auto_hide() -> PlatformResult<bool> {
    let state = on_helper("ABM_GETSTATE", tray_state)?;
    Ok(state & ABS_AUTOHIDE != 0)
}

/// Set the taskbar's auto-hide to `enabled` and confirm it with a read-back;
/// both messages share one [`SHELL_TIMEOUT`].
pub fn set_auto_hide(enabled: bool) -> PlatformResult<()> {
    let state = on_helper("ABM_SETSTATE", move || {
        set_tray_state(enabled)?;
        tray_state()
    })?;
    if (state & ABS_AUTOHIDE != 0) == enabled {
        Ok(())
    } else {
        Err(PlatformError::Other(format!(
            "the taskbar did not take auto-hide {}",
            if enabled { "on" } else { "off" }
        )))
    }
}

/// Run `call` on a helper thread and wait for its answer for at most
/// [`SHELL_TIMEOUT`] (see the module documentation). `what` names the
/// message for the log.
fn on_helper(what: &'static str, call: impl FnOnce() -> Answer + Send + 'static) -> Answer {
    let mut abandoned = abandoned();
    if let Some(earlier) = abandoned.as_ref() {
        match earlier.try_recv() {
            Err(TryRecvError::Empty) => {
                log::debug!(
                    "taskbar: {what} not sent: an earlier message is still waiting on Explorer"
                );
                return Err(PlatformError::Timeout);
            }
            Ok(_) | Err(TryRecvError::Disconnected) => {
                log::info!("taskbar: the message that timed out has returned; asking again");
                *abandoned = None;
            }
        }
    }
    let (answer_tx, answer_rx) = crossbeam_channel::bounded::<Answer>(1);
    std::thread::Builder::new()
        .name("stem-taskbar".into())
        .spawn(move || {
            // Capacity 1 and one send: this never blocks. After a timeout
            // the receiver waits in `ABANDONED` until a later call reads it.
            let _ = answer_tx.send(call());
        })
        .map_err(|e| PlatformError::Other(format!("could not start the taskbar thread: {e}")))?;
    match answer_rx.recv_timeout(SHELL_TIMEOUT) {
        Ok(answer) => answer,
        Err(RecvTimeoutError::Timeout) => {
            log::warn!(
                "taskbar: {what} got no answer from Explorer within {SHELL_TIMEOUT:?}; the \
                 call is abandoned (its thread ends when Explorer answers, or with the process)"
            );
            *abandoned = Some(answer_rx);
            Err(PlatformError::Timeout)
        }
        Err(RecvTimeoutError::Disconnected) => Err(PlatformError::Other(
            "the taskbar thread ended without an answer".into(),
        )),
    }
}

/// The `ABS_*` state bits from the tray. Helper thread.
fn tray_state() -> Answer {
    let tray = tray_window()?;
    let mut data = app_bar_data();
    // SAFETY: `data` is a complete `APPBARDATA` with `cbSize` set, alive for
    // the call; `ABM_GETSTATE` reads `cbSize` only and the shell does not
    // keep the pointer.
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) };
    // The tray that was asked must still be there: a tray that went away
    // meanwhile makes the shell answer `0`, which reads like "off".
    // SAFETY: IsWindow accepts any handle value.
    if !unsafe { IsWindow(Some(tray)) }.as_bool() {
        return Err(no_tray());
    }
    // The state bits fit in the low word; the cast cannot lose a set bit.
    Ok(state as u32)
}

/// `ABM_SETSTATE` with auto-hide `enabled`. Helper thread.
fn set_tray_state(enabled: bool) -> PlatformResult<()> {
    let _tray = tray_window()?;
    let mut data = app_bar_data();
    data.lParam = LPARAM(if enabled { ABS_AUTOHIDE as isize } else { 0 });
    // SAFETY: `data` is a complete `APPBARDATA` with `cbSize` set, alive for
    // the call; `ABM_SETSTATE` reads `cbSize` and `lParam` and the shell does
    // not keep the pointer.
    let _always_true = unsafe { SHAppBarMessage(ABM_SETSTATE, &mut data) };
    Ok(())
}

/// The tray window (`Shell_TrayWnd`), or an error when there is none.
fn tray_window() -> PlatformResult<HWND> {
    // SAFETY: both arguments are valid NUL-terminated strings or null; the
    // call reads the window list and sends no message.
    unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.map_err(|_| no_tray())
}

fn no_tray() -> PlatformError {
    PlatformError::Other(
        "there is no taskbar to ask (Shell_TrayWnd not found: Explorer is not running or is \
         restarting)"
            .into(),
    )
}

/// An `APPBARDATA` for a message that concerns the taskbar itself, not an
/// app bar of ours: no window, no edge, no rectangle.
fn app_bar_data() -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        ..Default::default()
    }
}

/// The abandoned helper's slot; a poisoned lock (a panic while held, which
/// nothing here can cause) still yields it.
fn abandoned() -> MutexGuard<'static, Option<Receiver<Answer>>> {
    ABANDONED.lock().unwrap_or_else(PoisonError::into_inner)
}
