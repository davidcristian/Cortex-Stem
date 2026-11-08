//! The stock taskbar's auto-hide state.
//!
//! Contract:
//! - [`auto_hide`] -> `SHAppBarMessage(ABM_GETSTATE)`: whether Windows' own
//!   *Automatically hide the taskbar* is on (`ABS_AUTOHIDE` in the returned
//!   state bits). `ABS_ALWAYSONTOP` is not reported since Windows 7 (the
//!   taskbar is always on top) and is ignored here.
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
//! Both calls are messages to the tray window (`Shell_TrayWnd`) that
//! `shell32` sends synchronously, so they wait on Explorer (bounded by the
//! shell's own timeout, not ours): the app layer runs them on Tauri's
//! blocking pool, never on the UI thread (`docs/development/windows-rules.md`). No window of ours
//! is involved (`hWnd` stays null), so nothing can call back into the dock
//! window's thread. With no tray at all (Explorer is not running)
//! `ABM_GETSTATE` reports `0`, i.e. "off", and `ABM_SETSTATE` changes
//! nothing: [`set_auto_hide`] then fails and the app layer retries later.

use windows::Win32::Foundation::LPARAM;
use windows::Win32::UI::Shell::{
    ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage,
};

use crate::{PlatformError, PlatformResult};

/// Whether the taskbar auto-hides right now. Cannot fail: the shell answers
/// with state bits and nothing else (no tray → `0` → off).
pub fn auto_hide() -> PlatformResult<bool> {
    Ok(state() & ABS_AUTOHIDE != 0)
}

/// Set the taskbar's auto-hide to `enabled` and confirm it with a read-back.
pub fn set_auto_hide(enabled: bool) -> PlatformResult<()> {
    let mut data = app_bar_data();
    data.lParam = LPARAM(if enabled { ABS_AUTOHIDE as isize } else { 0 });
    // SAFETY: `data` is a complete `APPBARDATA` with `cbSize` set, alive for
    // the call; `ABM_SETSTATE` reads `cbSize` and `lParam` and the shell does
    // not keep the pointer.
    let _always_true = unsafe { SHAppBarMessage(ABM_SETSTATE, &mut data) };
    if auto_hide()? == enabled {
        Ok(())
    } else {
        Err(PlatformError::Other(format!(
            "the taskbar did not take auto-hide {}",
            if enabled { "on" } else { "off" }
        )))
    }
}

/// The `ABS_*` state bits.
fn state() -> u32 {
    let mut data = app_bar_data();
    // SAFETY: `data` is a complete `APPBARDATA` with `cbSize` set, alive for
    // the call; `ABM_GETSTATE` reads `cbSize` only and the shell does not
    // keep the pointer.
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) };
    // The state bits fit in the low word; the cast cannot lose a set bit.
    state as u32
}

/// An `APPBARDATA` for a message that concerns the taskbar itself, not an
/// app bar of ours: no window, no edge, no rectangle.
fn app_bar_data() -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        ..Default::default()
    }
}
