//! Shared helpers for the Windows implementation.

use stem_core::WindowId;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::core::PCWSTR;

use crate::PlatformError;
/// `to_wide` and `from_wide` are pure (`rules::wide`, unit-tested on every
/// host); they are re-exported here, where the Windows code looks for them.
pub use crate::rules::wide::{from_wide, to_wide};

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

/// Pointer into a buffer produced by [`to_wide`].
pub fn pcwstr(buf: &[u16]) -> PCWSTR {
    PCWSTR(buf.as_ptr())
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
mod tests;

/// Owned process handle from `OpenProcess`; closed on drop.
#[derive(Debug)]
pub struct ProcessHandle(windows::Win32::Foundation::HANDLE);

impl ProcessHandle {
    /// `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`: enough for the image
    /// path and basic facts, and granted for most elevated processes too.
    /// Protected processes answer `PlatformError::AccessDenied`.
    pub fn open_limited(process_id: u32) -> crate::PlatformResult<Self> {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        // SAFETY: OpenProcess has no pointer arguments; a failure is reported
        // through the Result and the handle is owned by the guard on success.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }?;
        Ok(Self(handle))
    }

    pub fn raw(&self) -> windows::Win32::Foundation::HANDLE {
        self.0
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenProcess and is closed
        // exactly once, here.
        if let Err(e) = unsafe { windows::Win32::Foundation::CloseHandle(self.0) } {
            log::debug!("CloseHandle(process) failed: {e}");
        }
    }
}

/// `HRESULT` for a `WIN32_ERROR`, to compare a `windows::core::Error` code
/// against a specific Win32 error (`e.code() == hresult_of(ERROR_X)`).
pub fn hresult_of(error: windows::Win32::Foundation::WIN32_ERROR) -> windows::core::HRESULT {
    windows::core::HRESULT::from_win32(error.0)
}

// RAII guards for handles that must be released, plus small error helpers.
// The submodules are private; everything is re-exported from `util`.

pub use handles::{
    CoTaskWideString, OwnedBitmap, OwnedIcon, PropVariantGuard, ScreenDc, last_error,
};

mod handles;

pub use property_store::property_store_string;

mod property_store;

pub use dock_window_helpers::is_own_process_window;

mod dock_window_helpers;

pub use pe_resources::LoadedLibrary;

mod pe_resources;

impl ComApartment {
    /// [`ComApartment::sta`] for the process's multithreaded apartment
    /// (`COINIT_MULTITHREADED`): for a thread whose COM objects call back
    /// on threads of their own and need no message loop, such as the Core
    /// Audio interfaces of `volume` (`IMMDeviceEnumerator`,
    /// `IAudioEndpointVolume` and their notification callbacks).
    pub fn mta() -> Self {
        // SAFETY: as for `sta`: no preconditions beyond being called on the
        // thread that will use COM; a failure (RPC_E_CHANGED_MODE on a
        // thread already in an STA) leaves nothing to balance.
        let hr = unsafe { CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
        Self {
            initialised: hr.is_ok(),
        }
    }
}
