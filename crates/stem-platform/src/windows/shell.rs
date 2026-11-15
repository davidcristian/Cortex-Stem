//! Shell actions behind the fixed dock items.
//!
//! Contract for [`perform`]:
//! - `OpenStartMenu`: inject a `VK_LWIN` press/release with `SendInput`
//!   (there is no public API).
//! - `OpenTaskView`: `SendInput` Win+Tab.
//! - `ToggleShowDesktop`: `IShellDispatch::ToggleDesktop` via COM
//!   (`Shell.Application`), fallback Win+D.
//! - `OpenRecycleBin` / `EmptyRecycleBin`: `recycle_bin::{open, empty}`.
//! - `OpenQuickSettings` / `OpenNotificationCenter`: `SendInput`
//!   Win+A / Win+N, the documented shortcuts (Win down, the letter down and
//!   up, Win up); Windows has no public API that opens these flyouts. On the
//!   keyboard the same chord closes the open flyout again; whether a second
//!   click on the dock does too, or the flyout's light dismiss closes it on
//!   the press and the chord opens it again, is not documented.
//!
//! `SendInput` injects into the foreground thread's queue; it needs no window
//! of our own and works from a `WS_EX_NOACTIVATE` dock as long as this
//! process runs at the same integrity level as the desktop (UIPI). It does
//! not block: the call queues the events and returns. Microsoft documents
//! that it fails when UIPI blocks it (an elevated window has the foreground
//! and Stem is not elevated) and that neither the return value nor
//! `GetLastError` says UIPI was the cause; a count short of the events given
//! is logged at `warn` and returned as an error, and a refusal that still
//! reports every event cannot be seen here.

use stem_core::ShellAction;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_A, VK_D, VK_LWIN, VK_N, VK_TAB,
};
use windows::Win32::UI::Shell::{IShellDispatch4, Shell};

use super::util;
use crate::PlatformResult;

pub fn perform(action: ShellAction) -> PlatformResult<()> {
    match action {
        ShellAction::OpenRecycleBin => super::recycle_bin::open(),
        ShellAction::EmptyRecycleBin => super::recycle_bin::empty(),
        ShellAction::OpenStartMenu => send_chord(&[VK_LWIN]),
        ShellAction::OpenTaskView => send_chord(&[VK_LWIN, VK_TAB]),
        ShellAction::OpenQuickSettings => send_chord(&[VK_LWIN, VK_A]),
        ShellAction::OpenNotificationCenter => send_chord(&[VK_LWIN, VK_N]),
        ShellAction::ToggleShowDesktop => match toggle_desktop() {
            Ok(()) => Ok(()),
            Err(e) => {
                log::warn!("IShellDispatch::ToggleDesktop failed: {e}; sending Win+D");
                send_chord(&[VK_LWIN, VK_D])
            }
        },
    }
}

/// `Shell.Application` -> `ToggleDesktop` (introduced on `IShellDispatch4`),
/// the same entry point the stock taskbar's "show desktop" corner uses.
fn toggle_desktop() -> PlatformResult<()> {
    let _com = util::ComApartment::sta();
    // SAFETY: standard in-process activation of the Shell object.
    let shell: IShellDispatch4 = unsafe { CoCreateInstance(&Shell, None, CLSCTX_INPROC_SERVER) }?;
    // SAFETY: plain COM call on a live object.
    unsafe { shell.ToggleDesktop() }?;
    Ok(())
}

/// Press every key in order, then release them in reverse, in one
/// `SendInput` call so the sequence cannot interleave with real input.
fn send_chord(keys: &[VIRTUAL_KEY]) -> PlatformResult<()> {
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|&k| key_input(k, false))
        .chain(keys.iter().rev().map(|&k| key_input(k, true)))
        .collect();
    // SAFETY: `inputs` is a slice of fully initialised INPUT structures and
    // `cbsize` is their size; SendInput copies them.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        let err = util::last_error();
        log::warn!(
            "SendInput injected {sent} of {} events: {err}",
            inputs.len()
        );
        return Err(err);
    }
    Ok(())
}

fn key_input(key: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
