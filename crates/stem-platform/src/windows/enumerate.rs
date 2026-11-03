//! Window enumeration and eligibility.
//!
//! Contract:
//! - [`enumerate_taskbar_windows`] returns every top-level window that the
//!   stock taskbar would show, as [`RawWindow`] facts, in Z order (top first).
//! - [`query_window`] returns `Ok(None)` for windows that do not qualify.
//! - [`is_taskbar_candidate`] is the pure eligibility rule (visible, not a
//!   tool window, no owner unless `WS_EX_APPWINDOW`, not cloaked unless we
//!   want to report it, has a title or is a known UWP frame, ...).
//! - [`window_app_user_model_id`] reads `PKEY_AppUserModel_ID` from the
//!   window's property store (used for grouping and Store apps).
//!
//! All calls must be bounded: use `SendMessageTimeoutW`, never `SendMessageW`.

use stem_core::RawWindow;
use windows::Win32::Foundation::HWND;

use crate::{PlatformError, PlatformResult};

pub fn enumerate_taskbar_windows() -> PlatformResult<Vec<RawWindow>> {
    Err(PlatformError::Unsupported("window enumeration"))
}

pub fn query_window(_hwnd: HWND) -> PlatformResult<Option<RawWindow>> {
    Err(PlatformError::Unsupported("window enumeration"))
}

pub fn is_taskbar_candidate(_hwnd: HWND) -> bool {
    false
}

pub fn window_app_user_model_id(_hwnd: HWND) -> Option<String> {
    None
}
