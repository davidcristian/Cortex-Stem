//! The backdrop windows' class and its window procedure.

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, HTTRANSPARENT, MA_NOACTIVATE, MINMAXINFO, RegisterClassExW, UnregisterClassW,
    WM_DPICHANGED, WM_GETMINMAXINFO, WM_MOUSEACTIVATE, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST,
    WNDCLASSEXW,
};

use crate::PlatformResult;
use crate::windows::util;

const CLASS_NAME: &str = "StemDockBackdrop";

/// The registered window class; unregistered on drop.
pub(super) struct WindowClass {
    pub(super) name: Vec<u16>,
    pub(super) instance: HINSTANCE,
}

impl WindowClass {
    pub(super) fn register() -> PlatformResult<Self> {
        // SAFETY: a NULL module name asks for our own executable's handle.
        let module = unsafe { GetModuleHandleW(None) }?;
        let instance = HINSTANCE::from(module);
        let name = util::to_wide(CLASS_NAME);
        // SAFETY: GetStockObject has no preconditions; a stock brush is
        // never deleted, so the class may keep it for its lifetime.
        let black = unsafe { GetStockObject(BLACK_BRUSH) };
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(backdrop_wndproc),
            hInstance: instance,
            // The "sheet of glass": a black client area under an extended
            // frame lets the material through.
            hbrBackground: HBRUSH(black.0),
            lpszClassName: util::pcwstr(&name),
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised and the name buffer outlives
        // the call.
        if unsafe { RegisterClassExW(&class) } == 0 {
            let err = windows::core::Error::from_thread();
            if err.code() != util::hresult_of(ERROR_CLASS_ALREADY_EXISTS) {
                return Err(err.into());
            }
            // Left over from an earlier thread in this process; the
            // procedure is the same static function, so reusing it is fine.
        }
        Ok(Self { name, instance })
    }
}

impl Drop for WindowClass {
    fn drop(&mut self) {
        let name = util::pcwstr(&self.name);
        // SAFETY: the name buffer is alive; every window of the class was
        // destroyed before (field order of `Backdrops`).
        if let Err(e) = unsafe { UnregisterClassW(name, Some(self.instance)) } {
            log::debug!("UnregisterClassW(backdrop) failed: {e}");
        }
    }
}

unsafe extern "system" fn backdrop_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // The whole window is client area: the caption style is only there
        // so DWM accepts the backdrop type (A3).
        WM_NCCALCSIZE if wparam.0 != 0 => LRESULT(0),
        // Always "active" for DWM, so the material stays live on a window
        // that is never activated (A6). lParam is passed on unchanged.
        // SAFETY: forwarding our own arguments, with wParam forced to TRUE,
        // to the default procedure.
        WM_NCACTIVATE => unsafe { DefWindowProcW(hwnd, message, WPARAM(1), lparam) },
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        // The app layer sends the panel again at the new scale; the window
        // must not resize itself to keep a logical size.
        WM_DPICHANGED => LRESULT(0),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_GETMINMAXINFO => {
            let info = lparam.0 as *mut MINMAXINFO;
            if !info.is_null() {
                // SAFETY: for WM_GETMINMAXINFO the system passes a MINMAXINFO
                // that is valid for the duration of the message and is meant
                // to be modified by the window.
                let info = unsafe { &mut *info };
                info.ptMinTrackSize = POINT { x: 1, y: 1 };
            }
            LRESULT(0)
        }
        // SAFETY: forwarding the arguments we were given to the default
        // procedure.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
