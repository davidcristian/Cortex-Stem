//! Monitors, work areas and DPI.
//!
//! Contract:
//! - [`enumerate_monitors`] uses `EnumDisplayMonitors` + `GetMonitorInfoW` +
//!   `GetDpiForMonitor` and returns physical-pixel rectangles with the scale
//!   factor. `id` must be stable across calls while the monitor is attached
//!   (use the device name, e.g. `\\.\DISPLAY1`).
//! - [`monitor_for_window`] uses `MonitorFromWindow(MONITOR_DEFAULTTONEAREST)`.
//! - The process must be Per-Monitor-V2 DPI aware; Tauri sets this, but
//!   [`ensure_dpi_awareness`] is called defensively at startup.

use stem_core::{MonitorId, MonitorInfo};
use windows::Win32::Foundation::HWND;

use crate::{PlatformError, PlatformResult};

pub fn enumerate_monitors() -> PlatformResult<Vec<MonitorInfo>> {
    Err(PlatformError::Unsupported("monitor enumeration"))
}

pub fn monitor_for_window(_hwnd: HWND) -> Option<MonitorId> {
    None
}

pub fn ensure_dpi_awareness() {}
