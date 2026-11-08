//! Native message boxes.
//!
//! Contract:
//! - [`ask_yes_no`] -> `MessageBoxW(NULL, text, title, MB_YESNO |
//!   MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST)`: `IDYES` is
//!   `Ok(true)`; `IDNO`, and `IDCANCEL` (the box closed with Escape or its
//!   close button, which `MB_YESNO` has none of, so only a session end
//!   produces it) are `Ok(false)`; a `0` result is the call failing and
//!   becomes the thread's last error.
//!
//! The box has **no owner window** on purpose: the dock window is
//! `WS_EX_NOACTIVATE` and must not become the owner of an activating
//! dialog, and the settings window may not exist. `MB_SETFOREGROUND |
//! MB_TOPMOST` put the box above the dock (which sits in the topmost band)
//! and in front of whatever the user is doing, which is what a
//! once-per-crash report wants; a user who has not clicked anything else
//! since start also gets it focused.
//!
//! `MessageBoxW` creates the box on the calling thread and runs its own
//! message loop until the user answers, so the call **blocks for as long
//! as the box is up**: the app layer calls it from Tauri's blocking pool,
//! never from the UI thread or the service worker
//! (`docs/development/windows-rules.md`). The thread needs no COM
//! apartment.

use windows::Win32::UI::WindowsAndMessaging::{
    IDCANCEL, IDNO, IDYES, MB_ICONWARNING, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
    MESSAGEBOX_RESULT, MessageBoxW,
};

use super::util;
use crate::{PlatformError, PlatformResult};

/// Show a topmost *Yes* / *No* box and wait for the answer.
pub fn ask_yes_no(title: &str, text: &str) -> PlatformResult<bool> {
    let title_w = util::to_wide(title);
    let text_w = util::to_wide(text);
    // SAFETY: both buffers are NUL-terminated and outlive the call; no owner
    // window is passed, so no handle can be stale; the function has no
    // other pointer arguments.
    let answer = unsafe {
        MessageBoxW(
            None,
            util::pcwstr(&text_w),
            util::pcwstr(&title_w),
            MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST,
        )
    };
    match answer {
        IDYES => Ok(true),
        IDNO | IDCANCEL => Ok(false),
        MESSAGEBOX_RESULT(0) => Err(util::last_error()),
        other => Err(PlatformError::Other(format!(
            "MessageBoxW answered {}, which is neither Yes nor No",
            other.0
        ))),
    }
}
