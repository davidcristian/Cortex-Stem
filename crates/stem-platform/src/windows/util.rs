//! Shared helpers for the Windows implementation.

use stem_core::WindowId;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::core::PCWSTR;

use crate::PlatformError;

/// `WindowId` -> `HWND`. The id is the raw handle value.
pub fn hwnd(id: WindowId) -> HWND {
    HWND(id.0 as usize as *mut core::ffi::c_void)
}

/// `HWND` -> `WindowId`.
pub fn window_id(hwnd: HWND) -> WindowId {
    WindowId(hwnd.0 as usize as u64)
}

impl From<windows::core::Error> for PlatformError {
    fn from(err: windows::core::Error) -> Self {
        const E_ACCESSDENIED: u32 = 0x8007_0005;
        let code = err.code().0 as u32;
        if code == E_ACCESSDENIED {
            return PlatformError::AccessDenied;
        }
        PlatformError::Os {
            code,
            message: err.message(),
        }
    }
}

/// Build a NUL-terminated UTF-16 buffer for a `PCWSTR` argument.
/// Keep the returned `Vec` alive for as long as the pointer is used.
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Pointer into a buffer produced by [`to_wide`].
pub fn pcwstr(buf: &[u16]) -> PCWSTR {
    PCWSTR(buf.as_ptr())
}

/// Decode a UTF-16 buffer up to the first NUL (or its end).
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// RAII guard for a COM single-threaded apartment on the current thread.
/// Create one at the top of any thread that uses shell COM interfaces
/// (`IShellLinkW`, `IPropertyStore`, `IShellItemImageFactory`, ...).
#[derive(Debug)]
pub struct ComApartment {
    initialised: bool,
}

impl ComApartment {
    pub fn sta() -> Self {
        // SAFETY: CoInitializeEx has no preconditions beyond being called on
        // the thread that will use COM; S_FALSE (already initialised) and
        // RPC_E_CHANGED_MODE are handled by checking the result.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        Self {
            initialised: hr.is_ok(),
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.initialised {
            // SAFETY: balanced with the successful CoInitializeEx in `sta`.
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_round_trip() {
        let w = to_wide("Stem ✓");
        assert_eq!(w.last(), Some(&0));
        assert_eq!(from_wide(&w), "Stem ✓");
        assert_eq!(from_wide(&[0x44, 0x6f, 0x00, 0x63]), "Do");
    }

    #[test]
    fn window_id_round_trip() {
        let id = WindowId(0x0001_0203);
        assert_eq!(window_id(hwnd(id)), id);
    }
}
