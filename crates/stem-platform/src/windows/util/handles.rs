//! RAII guards for handles that must be released, and `last_error`.

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
