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
//! - `OpenDateTimeSettings` / `OpenPowerSettings` (the clock's and
//!   the battery's menus): open a page of the Settings app by its
//!   `ms-settings:` URI through `launch` (`ShellExecuteExW`, the way a
//!   `Uri` launch target opens). Microsoft's list of the scheme ("Launch
//!   Windows Settings", learn.microsoft.com) documents `ms-settings:dateandtime`
//!   as *Time and language > Date & time* and `ms-settings:powersleep` as
//!   *System > Power & sleep*, the Windows 10 name of the page Windows 11
//!   calls *Power & battery* on a machine with a battery and *Power* on one
//!   without. The documented battery pages
//!   (`ms-settings:batterysaver*`) exist only with a battery; `powersleep`
//!   exists on every machine.
//!
//! `SendInput` injects into the foreground thread's queue; it needs no window
//! of our own and works from a `WS_EX_NOACTIVATE` dock as long as this
//! process runs at the same integrity level as the desktop (UIPI). It does
//! not block: the call queues the events and returns. Microsoft documents
//! that it fails when UIPI blocks it (an elevated window has the foreground
//! and Stem is not elevated) and that neither the return value nor
//! `GetLastError` says UIPI was the cause; a count short of the events given
//! is logged at `warn` and returned as an error, and a refusal that still
//! reports every event cannot be seen here. A short count
//! above zero means the first events went in and the rest did not, so keys
//! can be down that no inserted event releases; [`send_chord`] releases them
//! before it returns the error (see `rules::shell::held_after`, which with
//! the page URIs is pure and unit-tested on every host).
//!
//! The events carry only the chord's own keys. A modifier the user holds
//! while clicking (Ctrl, Shift, Alt) is still down in the system's key
//! state, so Windows may take it as part of the chord: a Ctrl+click on the
//! Notification Center item can read as Win+Ctrl+N, which Microsoft lists
//! as *Open Narrator settings*. Nothing here releases held modifiers or
//! ignores modified clicks.

use stem_core::{LaunchTarget, ShellAction};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_A, VK_D, VK_LWIN, VK_N, VK_TAB,
};
use windows::Win32::UI::Shell::{IShellDispatch4, Shell};

use super::util;
use crate::PlatformResult;
use crate::rules::shell::{DATE_TIME_SETTINGS, POWER_SETTINGS, held_after};

pub fn perform(action: ShellAction) -> PlatformResult<()> {
    match action {
        ShellAction::OpenDateTimeSettings => open_settings_page(DATE_TIME_SETTINGS),
        ShellAction::OpenPowerSettings => open_settings_page(POWER_SETTINGS),
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

/// Open a page of the Settings app by its `ms-settings:` URI, as a `Uri`
/// launch target opens (`launch`: COM on this thread, then `ShellExecuteExW`
/// without UI, which returns once the shell has handed the URI on).
fn open_settings_page(uri: &str) -> PlatformResult<()> {
    let result = super::launch::launch(&LaunchTarget::Uri {
        uri: uri.to_owned(),
    });
    if let Err(e) = &result {
        log::warn!("opening {uri} failed: {e}");
    }
    result
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
///
/// On a short count the keys that the inserted events left down are released
/// with a second call before the error is returned, so a chord cut short
/// never leaves the Windows key held (which would turn every later keystroke
/// into a Windows shortcut). When only the Windows key's press went in, its
/// release is a tap of that key alone, which may open Start; that is the
/// lesser harm.
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
        // Read the error before anything else can overwrite it.
        let err = util::last_error();
        log::warn!(
            "SendInput injected {sent} of {} events: {err}",
            inputs.len()
        );
        release(held_after(keys, sent as usize));
        return Err(err);
    }
    Ok(())
}

/// Release `held` (in press order) in reverse, in one `SendInput` call, and
/// log the outcome; a failure here has nothing left to fall back on.
fn release(held: &[VIRTUAL_KEY]) {
    if held.is_empty() {
        return;
    }
    let inputs: Vec<INPUT> = held.iter().rev().map(|&k| key_input(k, true)).collect();
    // SAFETY: as in `send_chord`: fully initialised INPUT structures and
    // their size; SendInput copies them.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        log::warn!(
            "SendInput released {} key(s) the short chord left down",
            inputs.len()
        );
    } else {
        let err = util::last_error();
        log::warn!(
            "SendInput released {sent} of {} key(s) the short chord left down: {err}",
            inputs.len()
        );
    }
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
