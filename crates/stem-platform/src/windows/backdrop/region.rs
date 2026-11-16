//! The backdrop window's rounded region.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateRectRgn, CreateRoundRectRgn, DeleteObject, HRGN, SetWindowRgn,
};

use crate::rules::backdrop::RegionArgs;
use crate::{PlatformError, PlatformResult};

/// Give `hwnd` the region `args`. On success the system owns the region.
pub(super) fn set_region(hwnd: HWND, args: RegionArgs) -> PlatformResult<()> {
    let region: HRGN = if args.ellipse > 0 {
        // SAFETY: plain integers; the call returns a null handle on failure.
        unsafe {
            CreateRoundRectRgn(
                args.left,
                args.top,
                args.right,
                args.bottom,
                args.ellipse,
                args.ellipse,
            )
        }
    } else {
        // SAFETY: plain integers; the call returns a null handle on failure.
        unsafe { CreateRectRgn(args.left, args.top, args.right, args.bottom) }
    };
    if region.is_invalid() {
        return Err(PlatformError::Other(format!(
            "the region {args:?} could not be created"
        )));
    }
    // SAFETY: a window of this thread and a region this function created;
    // on success the system owns the region and it must not be used again.
    if unsafe { SetWindowRgn(hwnd, Some(region), true) } == 0 {
        // SAFETY: the call failed, so the region is still ours; it is
        // deleted exactly once, here.
        let _ = unsafe { DeleteObject(region.into()) };
        return Err(PlatformError::Other(format!(
            "SetWindowRgn({args:?}) failed"
        )));
    }
    Ok(())
}
