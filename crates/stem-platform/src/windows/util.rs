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
// Kept in a private module so the imports cannot clash with other sections;
// everything is re-exported from `util`.

pub use handles::{
    CoTaskWideString, OwnedBitmap, OwnedIcon, PropVariantGuard, ScreenDc, last_error,
};

mod handles {
    use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC};
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};
    use windows::core::PWSTR;

    use crate::PlatformError;

    /// The calling thread's last Win32 error as a [`PlatformError`].
    pub fn last_error() -> PlatformError {
        windows::core::Error::from_thread().into()
    }

    /// An `HICON` **we own** (from `PrivateExtractIconsW`,
    /// `SHGetFileInfoW(SHGFI_ICON)`, `CreateIconIndirect`, …), destroyed on
    /// drop. Never wrap icons returned by `WM_GETICON` or `GCLP_HICON`: those
    /// belong to the window and must not be destroyed.
    #[derive(Debug)]
    pub struct OwnedIcon(HICON);

    impl OwnedIcon {
        /// `None` for a null/invalid handle, so callers cannot forget to check.
        pub fn new(icon: HICON) -> Option<Self> {
            (!icon.is_invalid()).then_some(Self(icon))
        }

        pub fn handle(&self) -> HICON {
            self.0
        }
    }

    impl Drop for OwnedIcon {
        fn drop(&mut self) {
            // SAFETY: the handle is valid (checked in `new`) and owned by this
            // guard, which destroys it exactly once.
            if let Err(e) = unsafe { DestroyIcon(self.0) } {
                log::trace!("DestroyIcon failed: {e}");
            }
        }
    }

    /// An `HBITMAP` we own (from `GetIconInfo`,
    /// `IShellItemImageFactory::GetImage`, …), deleted on drop.
    #[derive(Debug)]
    pub struct OwnedBitmap(HBITMAP);

    impl OwnedBitmap {
        /// `None` for a null/invalid handle.
        pub fn new(bitmap: HBITMAP) -> Option<Self> {
            (!bitmap.is_invalid()).then_some(Self(bitmap))
        }

        pub fn handle(&self) -> HBITMAP {
            self.0
        }
    }

    impl Drop for OwnedBitmap {
        fn drop(&mut self) {
            // SAFETY: the handle is valid (checked in `new`), owned by this
            // guard, not selected into any DC, and deleted exactly once.
            let deleted = unsafe { DeleteObject(HGDIOBJ::from(self.0)) };
            if !deleted.as_bool() {
                log::trace!("DeleteObject(HBITMAP) failed");
            }
        }
    }

    /// The screen device context (`GetDC(NULL)`), released on drop.
    #[derive(Debug)]
    pub struct ScreenDc(HDC);

    impl ScreenDc {
        pub fn new() -> Result<Self, PlatformError> {
            // SAFETY: GetDC(NULL) has no preconditions; a null result is checked.
            let hdc = unsafe { GetDC(None) };
            if hdc.is_invalid() {
                return Err(PlatformError::Other("GetDC(NULL) failed".into()));
            }
            Ok(Self(hdc))
        }

        pub fn handle(&self) -> HDC {
            self.0
        }
    }

    impl Drop for ScreenDc {
        fn drop(&mut self) {
            // SAFETY: `self.0` came from GetDC(NULL) and is released exactly once.
            let released = unsafe { ReleaseDC(None, self.0) };
            if released == 0 {
                log::trace!("ReleaseDC failed");
            }
        }
    }

    /// A wide string allocated by COM (`CoTaskMemAlloc`), e.g. from
    /// `IShellItem::GetDisplayName` or `PropVariantToStringAlloc`; freed on drop.
    #[derive(Debug)]
    pub struct CoTaskWideString(PWSTR);

    impl CoTaskWideString {
        /// `None` for a null pointer.
        pub fn new(ptr: PWSTR) -> Option<Self> {
            (!ptr.is_null()).then_some(Self(ptr))
        }

        /// Decode up to the NUL terminator, replacing invalid UTF-16.
        pub fn to_string_lossy(&self) -> String {
            // SAFETY: the pointer is non-null (checked in `new`) and points at a
            // NUL-terminated string the API allocated for us.
            let wide = unsafe { self.0.as_wide() };
            String::from_utf16_lossy(wide)
        }
    }

    impl Drop for CoTaskWideString {
        fn drop(&mut self) {
            // SAFETY: the memory was allocated with CoTaskMemAlloc by the API
            // that returned it and is freed exactly once.
            unsafe { CoTaskMemFree(Some(self.0.0.cast_const().cast())) };
        }
    }

    /// A `PROPVARIANT` that is cleared (`PropVariantClear`) on drop.
    pub struct PropVariantGuard(PROPVARIANT);

    impl PropVariantGuard {
        pub fn new(value: PROPVARIANT) -> Self {
            Self(value)
        }

        pub fn as_ptr(&self) -> *const PROPVARIANT {
            &raw const self.0
        }
    }

    impl std::fmt::Debug for PropVariantGuard {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("PropVariantGuard(..)")
        }
    }

    impl Drop for PropVariantGuard {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a PROPVARIANT initialised by the API that
            // produced it (or zeroed); PropVariantClear releases whatever it holds.
            if let Err(e) = unsafe { PropVariantClear(&raw mut self.0) } {
                log::trace!("PropVariantClear failed: {e}");
            }
        }
    }
}

pub use property_store::property_store_string;

mod property_store {
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

    use super::{CoTaskWideString, PropVariantGuard};

    /// A string property of a property store, trimmed. `None` when the key
    /// is absent (`VT_EMPTY`), the value is blank, or it cannot be read as a
    /// string. The conversion goes through `PropVariantToStringAlloc`, so
    /// every caller (window property stores in `enumerate`, shortcut
    /// property stores in `identity`) accepts the same variant types
    /// (`VT_LPWSTR`, `VT_BSTR`, `VT_LPSTR`, ...). The caller must hold a COM
    /// apartment on this thread.
    pub fn property_store_string(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
        // SAFETY: `key` is a valid PROPERTYKEY; the returned PROPVARIANT is
        // owned by us and cleared by the guard.
        let value = unsafe { store.GetValue(key) }.ok()?;
        let value = PropVariantGuard::new(value);
        // SAFETY: `value` is a live PROPVARIANT; the returned string is
        // CoTaskMem-allocated and freed by the guard.
        let text = unsafe { PropVariantToStringAlloc(value.as_ptr()) }.ok()?;
        let text = CoTaskWideString::new(text)?.to_string_lossy();
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }
}

pub use dock_window_helpers::is_own_process_window;

mod dock_window_helpers {
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
}

pub use pe_resources::LoadedLibrary;

mod pe_resources {
    use windows::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW,
    };

    use crate::PlatformResult;

    /// A PE file mapped by `LoadLibraryExW` for reading its resources only
    /// (`LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE`: nothing
    /// of the file runs, no `DllMain`, no imports are resolved, and a
    /// 32-bit image maps in a 64-bit process); `FreeLibrary` on drop. The
    /// resource functions (`EnumResourceNamesW`, `FindResourceW`,
    /// `LoadResource`, …) accept the handle; `GetProcAddress` and friends
    /// do not.
    #[derive(Debug)]
    pub struct LoadedLibrary(HMODULE);

    impl LoadedLibrary {
        /// Map the file at `path`. The error is the loader's: file not
        /// found, access denied for a file the user cannot read, bad exe
        /// format for a file that is not a PE image.
        pub fn open_as_data(path: &str) -> PlatformResult<Self> {
            let wide = super::to_wide(path);
            // SAFETY: `wide` is NUL-terminated and outlives the call; the
            // data-file flags map the image without running any of its code;
            // the handle is owned by the guard on success.
            let module = unsafe {
                LoadLibraryExW(
                    super::pcwstr(&wide),
                    None,
                    LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
                )
            }?;
            Ok(Self(module))
        }

        pub fn handle(&self) -> HMODULE {
            self.0
        }
    }

    impl Drop for LoadedLibrary {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful LoadLibraryExW and is
            // freed exactly once, here; no pointer into the mapping outlives
            // the guard (`read_resource` in `icons` copies the bytes out).
            if let Err(e) = unsafe { FreeLibrary(self.0) } {
                log::trace!("FreeLibrary failed: {e}");
            }
        }
    }
}
