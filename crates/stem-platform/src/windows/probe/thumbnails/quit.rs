//! Stopping the probe: why it stops, the console control handler, and
//! waking the message loop from another thread.

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{FALSE, HWND, LPARAM, TRUE, WPARAM};
use windows::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::BOOL;

use super::WM_PROBE_WAKE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Quit {
    Typed = 1,
    CtrlC = 2,
    CtrlBreak = 3,
    ConsoleClosed = 4,
    CloseButton = 5,
    StdinClosed = 6,
    MessageLoop = 7,
    WorkerStuck = 8,
}

/// Why the probe stops (a `Quit` value), 0 while it runs.
static QUIT: AtomicU8 = AtomicU8::new(0);
/// The destination window's handle, for the console handler and the stdin
/// thread to wake the message loop; 0 when there is none.
pub(super) static DEST: AtomicIsize = AtomicIsize::new(0);
/// Set once every thumbnail is unregistered and every window destroyed.
pub(super) static CLEANED: AtomicBool = AtomicBool::new(false);
/// When the probe started: the source windows' clock.
pub(super) static START: OnceLock<Instant> = OnceLock::new();

pub(super) fn request_quit(reason: Quit) {
    let _ = QUIT.compare_exchange(0, reason as u8, Ordering::SeqCst, Ordering::SeqCst);
}

pub(super) fn quitting() -> bool {
    QUIT.load(Ordering::SeqCst) != 0
}

pub(super) fn quit_text() -> &'static str {
    match QUIT.load(Ordering::SeqCst) {
        1 => "q was typed",
        2 => "Ctrl+C",
        3 => "Ctrl+Break",
        4 => "the console was closed",
        5 => "the probe window's close button",
        6 => "stdin was closed",
        7 => "the message loop ended",
        8 => "the A6 worker thread did not finish",
        _ => "the probe finished",
    }
}

/// Wakes the destination's message loop from another thread.
pub(super) fn wake(dest: isize) {
    if dest == 0 {
        return;
    }
    let hwnd = HWND(dest as *mut c_void);
    // SAFETY: PostMessageW accepts any handle value and fails for a dead
    // one; the message carries no pointers.
    let _ = unsafe { PostMessageW(Some(hwnd), WM_PROBE_WAKE, WPARAM(0), LPARAM(0)) };
}

/// Console control handler: Ctrl+C, Ctrl+Break and closing the console stop
/// the probe through its normal clean-up instead of killing it. A Ctrl+C or
/// Ctrl+Break once the probe is already stopping (for example while a DWM
/// call hangs) is left to Windows' default handler, which ends the process;
/// Windows then frees every thumbnail and window the process had.
pub(super) unsafe extern "system" fn on_console_event(kind: u32) -> BOOL {
    let reason = match kind {
        CTRL_C_EVENT => Quit::CtrlC,
        CTRL_BREAK_EVENT => Quit::CtrlBreak,
        CTRL_CLOSE_EVENT => Quit::ConsoleClosed,
        _ => return FALSE,
    };
    if kind != CTRL_CLOSE_EVENT && quitting() {
        return FALSE;
    }
    request_quit(reason);
    wake(DEST.load(Ordering::SeqCst));
    if kind == CTRL_CLOSE_EVENT {
        // Windows ends the process when this handler returns: give the main
        // thread a moment to unregister and destroy first.
        let deadline = Instant::now() + Duration::from_secs(3);
        while !CLEANED.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    TRUE
}
