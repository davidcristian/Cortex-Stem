//! What the dock windows' subclass (on the main thread) calls: the UIPI
//! filter for the shell's messages, and the handlers that queue a command
//! for the thread and return.

use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, MSGFLT_ALLOW, RegisterWindowMessageW, WA_INACTIVE,
};
use windows::core::w;

use super::{CALLBACK_MESSAGE, Command, SENDER};

/// The `TaskbarCreated` message id, registered on first use.
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

/// Let the shell's AppBar messages ([`CALLBACK_MESSAGE`] and
/// `TaskbarCreated`) through UIPI to the dock window `hwnd`, so an elevated
/// Stem still hears them from the medium-integrity Explorer (the `appbar`
/// module docs).
/// `dock_window::prepare` calls it for each dock window; it sends nothing
/// and a failure is logged, never fatal (the band then does not follow the
/// shell's changes while elevated).
pub fn allow_shell_messages(hwnd: HWND) {
    let messages = [
        (CALLBACK_MESSAGE, "the AppBar callback message"),
        (taskbar_created_message(), "TaskbarCreated"),
    ];
    for (message, name) in messages {
        if message == 0 {
            continue;
        }
        // SAFETY: changes the message filter of a window of this process;
        // no out-structure is passed. A dead handle only makes it fail.
        let allowed = unsafe { ChangeWindowMessageFilterEx(hwnd, message, MSGFLT_ALLOW, None) };
        match allowed {
            Ok(()) => log::trace!(
                "dock window {:#x}: {name} ({message:#x}) allowed through UIPI",
                raw(hwnd)
            ),
            Err(e) => log::debug!(
                "dock window {:#x}: could not let {name} ({message:#x}) through UIPI: {e}; an \
                 elevated Stem would not hear it",
                raw(hwnd)
            ),
        }
    }
}

/// Queue `command` for the thread, if one runs. Never waits.
fn forward(command: Command) {
    if let Some(sender) = SENDER.lock().as_ref() {
        let _ = sender.send(command);
    }
}

fn raw(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

/// [`CALLBACK_MESSAGE`] reached a dock window.
pub fn on_callback(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    forward(Command::Notify {
        handle: raw(hwnd),
        code: wparam.0 as u32,
        arg: lparam.0,
    });
}

/// `WM_ACTIVATE` reached a dock window (`wParam`'s low word: `WA_*`).
pub fn on_activate(hwnd: HWND, wparam: WPARAM) {
    forward(Command::Activate {
        handle: raw(hwnd),
        active: (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE,
    });
}

/// `WM_WINDOWPOSCHANGED` reached a dock window.
pub fn on_window_pos_changed(hwnd: HWND) {
    forward(Command::WindowPosChanged(raw(hwnd)));
}

/// Whether `msg` is `TaskbarCreated` (known once a dock window was prepared
/// or the thread started; before that no bar exists that would need it).
pub fn is_taskbar_created(msg: u32) -> bool {
    TASKBAR_CREATED
        .get()
        .is_some_and(|&id| id != 0 && id == msg)
}

/// `TaskbarCreated` reached a dock window: the shell started again.
pub fn on_taskbar_created(hwnd: HWND) {
    forward(Command::TaskbarCreated(raw(hwnd)));
}

/// A dock window is being destroyed (`WM_NCDESTROY`).
pub fn on_destroyed(hwnd: HWND) {
    forward(Command::Destroyed(raw(hwnd)));
}

/// The id of the `TaskbarCreated` message the shell broadcasts to every
/// top-level window when it starts; 0 when it could not be registered.
pub(super) fn taskbar_created_message() -> u32 {
    *TASKBAR_CREATED.get_or_init(|| {
        // SAFETY: a valid NUL-terminated string; the call registers (or
        // looks up) a message name and sends nothing.
        unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }
    })
}
