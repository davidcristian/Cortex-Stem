//! A PE file mapped to read its resources.

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
