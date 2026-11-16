//! Which windows belong to this process.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

/// Whether `hwnd` belongs to the calling process (the dock window, the
/// settings window, the hidden helper windows). `false` for a dead
/// handle.
pub fn is_own_process_window(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    // SAFETY: `pid` is a live out-pointer; the call accepts any handle
    // value and returns 0 (leaving `pid` untouched) for a dead one.
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if thread == 0 {
        return false;
    }
    // SAFETY: GetCurrentProcessId has no preconditions.
    pid == unsafe { GetCurrentProcessId() }
}
