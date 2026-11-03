//! OS-level treatment of our own dock windows.
//!
//! Contract:
//! - [`prepare`] adjusts the extended style of the given `HWND`:
//!   `WS_EX_TOOLWINDOW` (hidden from Alt+Tab and the stock taskbar) and
//!   `WS_EX_NOACTIVATE` (clicks do not steal focus) when requested; sets
//!   `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND` and disables the
//!   DWM shadow (`DWMWA_NCRENDERING_POLICY`) when `no_system_decorations`.
//!   Must be called after Tauri created the window and before it is shown.
//! - [`set_hit_region`] subclasses the window (`SetWindowSubclass`) so that
//!   `WM_NCHITTEST` returns `HTTRANSPARENT` outside `region`; `None` removes
//!   the subclass. (M3)

use stem_core::Rect;

use crate::{DockWindowOptions, NativeWindowHandle, PlatformError, PlatformResult};

pub fn prepare(_handle: NativeWindowHandle, _options: DockWindowOptions) -> PlatformResult<()> {
    Err(PlatformError::Unsupported("dock window styling"))
}

pub fn set_hit_region(_handle: NativeWindowHandle, _region: Option<Rect>) -> PlatformResult<()> {
    Err(PlatformError::Unsupported("click-through"))
}
