//! Whether this Windows build has `DWMWA_SYSTEMBACKDROP_TYPE`.

use std::sync::OnceLock;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::core::w;

use crate::rules::backdrop::{MIN_BUILD, parse_build};
use crate::windows::util;

/// Whether this Windows build has `DWMWA_SYSTEMBACKDROP_TYPE` (see the
/// `backdrop` module docs). Decided and logged once per process.
pub fn supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| match os_build() {
        Some(build) if build >= MIN_BUILD => {
            log::debug!("Windows build {build}: DWMWA_SYSTEMBACKDROP_TYPE available");
            true
        }
        Some(build) => {
            log::info!(
                "native backdrop refused: Windows build {build} has no DWMWA_SYSTEMBACKDROP_TYPE (it needs build {MIN_BUILD}); the panel keeps its CSS look"
            );
            false
        }
        None => {
            log::debug!("the Windows build number could not be read; trying the native backdrop");
            true
        }
    })
}

/// `CurrentBuildNumber` from the registry; `None` when it cannot be read.
fn os_build() -> Option<u32> {
    let mut buf = [0u16; 32];
    let mut size = u32::try_from(size_of_val(&buf)).unwrap_or(0);
    // SAFETY: `buf` is a live, writable buffer of `size` bytes; RRF_RT_REG_SZ
    // makes the call fail instead of writing a value of another type; the
    // key and value names are static NUL-terminated literals.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        log::debug!("RegGetValueW(CurrentBuildNumber) failed: {}", status.0);
        return None;
    }
    let chars = (size as usize / size_of::<u16>()).min(buf.len());
    parse_build(&util::from_wide(&buf[..chars]))
}
