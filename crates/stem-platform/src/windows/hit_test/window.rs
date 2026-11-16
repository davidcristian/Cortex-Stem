//! The message-only window of the hit-test thread, which receives raw
//! mouse input and the poll timer.

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    RAWINPUTDEVICE, RIDEV_INPUTSINK, RIDEV_REMOVE, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, KillTimer, RegisterClassExW,
    SetTimer, UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW,
};

use super::{FALLBACK_POLL, SAFETY_POLL};
use crate::windows::util;
use crate::{PlatformError, PlatformResult};

const CLASS_NAME: &str = "DockPlatformHitTest";
const WINDOW_TITLE: &str = "Stem platform hit test";
/// Window timer id of the safety / fallback poll.
const POLL_TIMER_ID: usize = 1;
/// HID usage page "Generic Desktop Controls" and its "Mouse" usage (HID
/// Usage Tables §4); literal so no extra `windows` feature is needed.
const HID_USAGE_PAGE_GENERIC: u16 = 0x01;
const HID_USAGE_GENERIC_MOUSE: u16 = 0x02;

/// The message-only window that receives raw input and the poll timer.
pub(super) struct TrackerWindow {
    hwnd: HWND,
    class_name: Vec<u16>,
    instance: HINSTANCE,
    /// Raw mouse input is registered on this window.
    pub(super) raw_input: bool,
}

impl TrackerWindow {
    pub(super) fn create() -> PlatformResult<Self> {
        // SAFETY: a NULL module name asks for our own executable's handle.
        let module = unsafe { GetModuleHandleW(None) }?;
        let instance = HINSTANCE::from(module);
        let class_name = util::to_wide(CLASS_NAME);
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(tracker_wndproc),
            hInstance: instance,
            lpszClassName: util::pcwstr(&class_name),
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised and the name buffer outlives the call.
        if unsafe { RegisterClassExW(&class) } == 0 {
            let err = windows::core::Error::from_thread();
            if err.code() != util::hresult_of(ERROR_CLASS_ALREADY_EXISTS) {
                return Err(err.into());
            }
            // Left over from a previous start in this process; the procedure
            // is the same static function, so reusing the class is fine.
        }
        let title = util::to_wide(WINDOW_TITLE);
        // SAFETY: the class is registered; the string buffers outlive the
        // call; HWND_MESSAGE makes it a message-only window.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                util::pcwstr(&class_name),
                util::pcwstr(&title),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )
        }?;
        let mut window = Self {
            hwnd,
            class_name,
            instance,
            raw_input: false,
        };
        window.raw_input = window.register_raw_input();
        let poll = if window.raw_input {
            SAFETY_POLL
        } else {
            FALLBACK_POLL
        };
        let millis = u32::try_from(poll.as_millis()).unwrap_or(u32::MAX).max(1);
        // SAFETY: a timer on a window of this thread, without a callback:
        // WM_TIMER is delivered to this thread's queue.
        if unsafe { SetTimer(Some(hwnd), POLL_TIMER_ID, millis, None) } == 0 {
            let err = util::last_error();
            if window.raw_input {
                log::warn!(
                    "hit-test tracker: SetTimer({millis} ms) failed ({err}); raw input only"
                );
            } else {
                return Err(PlatformError::Other(format!(
                    "hit-test tracker has neither raw input nor a timer: {err}"
                )));
            }
        }
        Ok(window)
    }

    /// Mouse raw input delivered to this window even in the background.
    fn register_raw_input(&self) -> bool {
        let devices = [RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: HID_USAGE_GENERIC_MOUSE,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: self.hwnd,
        }];
        // SAFETY: the array is live for the call and cbSize is the size of
        // its element type.
        match unsafe { RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) } {
            Ok(()) => true,
            Err(e) => {
                log::warn!("RegisterRawInputDevices(mouse, INPUTSINK) failed: {e}");
                false
            }
        }
    }
}

impl Drop for TrackerWindow {
    fn drop(&mut self) {
        if self.raw_input {
            let devices = [RAWINPUTDEVICE {
                usUsagePage: HID_USAGE_PAGE_GENERIC,
                usUsage: HID_USAGE_GENERIC_MOUSE,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: HWND::default(),
            }];
            // SAFETY: as in `register_raw_input`; RIDEV_REMOVE requires a
            // NULL target window.
            let removed =
                unsafe { RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) };
            if let Err(e) = removed {
                log::debug!("RegisterRawInputDevices(RIDEV_REMOVE) failed: {e}");
            }
        }
        // SAFETY: the timer belongs to this window; a missing timer only
        // makes the call fail.
        if let Err(e) = unsafe { KillTimer(Some(self.hwnd), POLL_TIMER_ID) } {
            log::trace!("KillTimer(hit-test poll) failed: {e}");
        }
        // SAFETY: the window was created by this thread and is destroyed once.
        if let Err(e) = unsafe { DestroyWindow(self.hwnd) } {
            log::warn!("DestroyWindow(hit-test window) failed: {e}");
        }
        let class_name = util::pcwstr(&self.class_name);
        // SAFETY: the class name buffer is alive; the only window of the
        // class has just been destroyed.
        if let Err(e) = unsafe { UnregisterClassW(class_name, Some(self.instance)) } {
            log::debug!("UnregisterClassW(hit-test) failed: {e}");
        }
    }
}

/// Window procedure of the message-only window. The loop already acted on
/// `WM_INPUT` and `WM_TIMER` before dispatching; the default procedure
/// releases the raw-input payload.
unsafe extern "system" fn tracker_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: forwarding the arguments we were given to the default procedure.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
