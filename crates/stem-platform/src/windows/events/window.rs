//! The hidden window of the events thread.
//!
//! **Hidden top-level window instead of a message-only window.**
//! `WM_SETTINGCHANGE`, `WM_DISPLAYCHANGE`, `WM_THEMECHANGED`,
//! `WM_DWMCOLORIZATIONCOLORCHANGED` and `WM_POWERBROADCAST`
//! (`PBT_APMPOWERSTATUSCHANGE`, the resume codes) are broadcasts to
//! *top-level* windows, and
//! message-only windows (`HWND_MESSAGE` parent) are documented not to receive
//! broadcasts. The window is therefore an ordinary top-level window that is
//! never shown (`WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, 0×0). It belongs to
//! our own process and is invisible, so `enumerate` rejects it.

use std::sync::OnceLock;

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DeregisterShellHookWindow, DestroyWindow,
    PBT_POWERSETTINGCHANGE, RegisterClassExW, RegisterShellHookWindow, RegisterWindowMessageW,
    UnregisterClassW, WM_DISPLAYCHANGE, WM_DWMCOLORIZATIONCOLORCHANGED, WM_POWERBROADCAST,
    WM_SETTINGCHANGE, WM_THEMECHANGED, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_OVERLAPPED,
};

use super::dispatch::{
    MSG_DISPLAYCHANGE, MSG_POWERCHANGE, MSG_SETTINGCHANGE, MSG_SHELLHOOK, MSG_THEMECHANGE,
    SETTING_AREA_INTL, post_to_self,
};
use super::power_status::power_setting_saver;
use crate::PlatformResult;
use crate::rules::locale as locale_rules;
use crate::rules::power as power_rules;
use crate::windows::util;

/// The most UTF-16 units of a `WM_SETTINGCHANGE` area name the window
/// procedure reads, up to its NUL. The one name it looks for is
/// `intl`; a longer name is not read to its end and is no match.
pub(super) const SETTING_AREA_MAX: usize = 32;

const CLASS_NAME: &str = "DockPlatformEvents";
const WINDOW_TITLE: &str = "Stem platform events";
const SHELL_HOOK_MESSAGE_NAME: &str = "SHELLHOOK";

/// `RegisterWindowMessageW("SHELLHOOK")`: a per-process constant.
static SHELL_HOOK_MESSAGE: OnceLock<u32> = OnceLock::new();

/// The hidden window that receives broadcasts and shell-hook messages.
pub(super) struct EventWindow {
    pub(super) hwnd: HWND,
    class_name: Vec<u16>,
    instance: HINSTANCE,
    shell_hook_registered: bool,
}

impl EventWindow {
    pub(super) fn create() -> PlatformResult<Self> {
        // SAFETY: a NULL module name asks for our own executable's handle.
        let module = unsafe { GetModuleHandleW(None) }?;
        let instance = HINSTANCE::from(module);
        let class_name = util::to_wide(CLASS_NAME);
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(event_wndproc),
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
        // SAFETY: the class is registered; the string buffers outlive the call;
        // no parent, menu or creation parameter is needed.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                util::pcwstr(&class_name),
                util::pcwstr(&title),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
        }?;
        let mut window = Self {
            hwnd,
            class_name,
            instance,
            shell_hook_registered: false,
        };
        let message_name = util::to_wide(SHELL_HOOK_MESSAGE_NAME);
        // SAFETY: the name buffer outlives the call.
        let message = unsafe { RegisterWindowMessageW(util::pcwstr(&message_name)) };
        if message == 0 {
            log::warn!(
                "RegisterWindowMessageW(SHELLHOOK) failed; flash/activation shell events off"
            );
        } else {
            let _ = SHELL_HOOK_MESSAGE.set(message);
            // SAFETY: `hwnd` is a live window owned by this thread.
            window.shell_hook_registered = unsafe { RegisterShellHookWindow(hwnd) }.as_bool();
            if !window.shell_hook_registered {
                log::warn!("RegisterShellHookWindow failed; flash/activation shell events off");
            }
        }
        Ok(window)
    }
}

impl Drop for EventWindow {
    fn drop(&mut self) {
        if self.shell_hook_registered {
            // SAFETY: balances the successful RegisterShellHookWindow on this window.
            if !unsafe { DeregisterShellHookWindow(self.hwnd) }.as_bool() {
                log::debug!("DeregisterShellHookWindow failed");
            }
        }
        // SAFETY: the window was created by this thread and is destroyed once.
        if let Err(e) = unsafe { DestroyWindow(self.hwnd) } {
            log::warn!("DestroyWindow(events window) failed: {e}");
        }
        let class_name = util::pcwstr(&self.class_name);
        // SAFETY: the class name buffer is alive; the only window of the class
        // has just been destroyed.
        let unregistered = unsafe { UnregisterClassW(class_name, Some(self.instance)) };
        if let Err(e) = unregistered {
            log::debug!("UnregisterClassW failed: {e}");
        }
    }
}

/// Whether the area a `WM_SETTINGCHANGE` names is `intl`:
/// `rules::locale::is_intl_area` of the string behind `lparam`, read while the
/// window procedure runs, unit by unit up to its NUL and at most
/// [`SETTING_AREA_MAX`] units, without assuming alignment; `false` for a
/// NULL `lparam` and for a name not ended within the bound.
///
/// # Safety
///
/// `lparam` is NULL or points at a readable, NUL-terminated UTF-16 string,
/// as the `lParam` of a `WM_SETTINGCHANGE` does while it is being handled.
pub(super) unsafe fn setting_is_intl(lparam: LPARAM) -> bool {
    let base = lparam.0 as *const u16;
    if base.is_null() {
        return false;
    }
    let mut area = [0u16; SETTING_AREA_MAX];
    for (i, slot) in area.iter_mut().enumerate() {
        let at = base.wrapping_add(i);
        // SAFETY: the caller guarantees a NUL-terminated string; no unit
        // before this one was a NUL, so unit `i` is part of the string (at
        // most its terminator) and readable. Read unaligned.
        let unit = unsafe { at.read_unaligned() };
        if unit == 0 {
            return locale_rules::is_intl_area(&area[..i]);
        }
        *slot = unit;
    }
    false
}

/// Window procedure of the hidden window: forward the interesting messages
/// to the loop as thread messages, default-handle everything else.
unsafe extern "system" fn event_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SETTINGCHANGE => {
            // lParam may point at a string that is only valid during this
            // call; the action code travels on, and whether the string named
            // the `intl` area, read here.
            // SAFETY: the lParam of WM_SETTINGCHANGE is NULL or a
            // NUL-terminated string valid while this runs: the system copies
            // it into this process for a sender elsewhere, and refuses to
            // post the message (it can only be sent).
            let intl = unsafe { setting_is_intl(lparam) };
            let area = if intl { SETTING_AREA_INTL } else { 0 };
            post_to_self(MSG_SETTINGCHANGE, wparam, LPARAM(area));
            LRESULT(0)
        }
        WM_DWMCOLORIZATIONCOLORCHANGED | WM_THEMECHANGED => {
            post_to_self(MSG_THEMECHANGE, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            post_to_self(MSG_DISPLAYCHANGE, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        // The power source, the battery's charge, battery saver or a
        // resume. The loop asks for the whole status again; lParam (a
        // POWERBROADCAST_SETTING for a setting change) is only valid during
        // this call, so the one value taken from it, battery saver's new
        // state, is copied out here and posted with the code. TRUE is the
        // documented answer to a processed broadcast.
        WM_POWERBROADCAST if power_rules::power_broadcast_cause(wparam.0 as u32).is_some() => {
            let saver = if wparam.0 as u32 == PBT_POWERSETTINGCHANGE {
                // SAFETY: for PBT_POWERSETTINGCHANGE, lParam is the
                // POWERBROADCAST_SETTING the system passes with this call
                // (with its DataLength bytes of data), valid while it runs.
                unsafe { power_setting_saver(lparam) }
            } else {
                None
            };
            post_to_self(
                MSG_POWERCHANGE,
                wparam,
                LPARAM(power_rules::saver_to_lparam(saver)),
            );
            LRESULT(1)
        }
        m if SHELL_HOOK_MESSAGE.get() == Some(&m) => {
            post_to_self(MSG_SHELLHOOK, wparam, lparam);
            LRESULT(0)
        }
        // SAFETY: forwarding the arguments we were given to the default procedure.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
