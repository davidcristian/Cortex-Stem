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
//!
//! Implementation notes:
//! - `EnumDisplayMonitors` enumerates into a `Vec<HMONITOR>` addressed by the
//!   `LPARAM`; the callback does nothing else. Monitor facts are read after
//!   the enumeration returns so a failing monitor cannot abort the others.
//! - Scale factor = effective DPI / 96 (`MDT_EFFECTIVE_DPI`), which is what
//!   the shell uses for window scaling; `96` is assumed when the query fails.
//! - `MonitorInfo::name` is the device name without the `\\.\` prefix; the
//!   `id` keeps the full device name so it is unambiguous.

use stem_core::{MonitorId, MonitorInfo, Rect};
use windows::Win32::Foundation::{E_ACCESSDENIED, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MONITORINFOEXW, MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::BOOL;

use super::util;
use crate::{PlatformError, PlatformResult};

const BASE_DPI: f64 = 96.0;

pub fn enumerate_monitors() -> PlatformResult<Vec<MonitorInfo>> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    let lparam = LPARAM(std::ptr::from_mut(&mut handles) as isize);
    // SAFETY: `collect_monitor` only pushes onto the Vec addressed by
    // `lparam`, which outlives the (synchronous) enumeration.
    unsafe { EnumDisplayMonitors(None, None, Some(collect_monitor), lparam) }.ok()?;

    let mut out = Vec::with_capacity(handles.len());
    for handle in handles {
        match monitor_info(handle) {
            Some(info) => out.push(info),
            None => log::warn!("GetMonitorInfoW failed for monitor {:?}", handle.0),
        }
    }
    if out.is_empty() {
        return Err(PlatformError::Other(
            "EnumDisplayMonitors reported no usable monitor".into(),
        ));
    }
    Ok(out)
}

pub fn monitor_for_window(hwnd: HWND) -> Option<MonitorId> {
    // SAFETY: MonitorFromWindow accepts any handle value; NEAREST never
    // returns NULL for a live window, and NULL is handled below.
    let handle = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if handle.is_invalid() {
        return None;
    }
    device_name(handle).map(MonitorId)
}

/// Opt the process into Per-Monitor-V2 DPI awareness. Tauri's manifest
/// already does this; the call then fails with `E_ACCESSDENIED` ("already
/// set"), which is the expected outcome and is only logged at debug level.
pub fn ensure_dpi_awareness() {
    // SAFETY: no pointer arguments; the context constant is a documented value.
    match unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) } {
        Ok(()) => log::info!("process DPI awareness set to per-monitor v2"),
        Err(e) if e.code() == E_ACCESSDENIED => {
            log::debug!("process DPI awareness already set (manifest or host)");
        }
        Err(e) => log::warn!("SetProcessDpiAwarenessContext failed: {e}"),
    }
}

unsafe extern "system" fn collect_monitor(
    handle: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let list = lparam.0 as *mut Vec<HMONITOR>;
    // SAFETY: `lparam` is the pointer `enumerate_monitors` passed to
    // EnumDisplayMonitors; the Vec is alive and not otherwise borrowed while
    // the enumeration runs on this thread.
    let list = unsafe { &mut *list };
    list.push(handle);
    BOOL(1)
}

/// `GetMonitorInfoW` with the `MONITORINFOEXW` variant (device name included).
fn monitor_info_ex(handle: HMONITOR) -> Option<MONITORINFOEXW> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let ptr = std::ptr::from_mut(&mut info).cast::<MONITORINFO>();
    // SAFETY: `info` is a MONITORINFOEXW whose first member is MONITORINFO and
    // whose cbSize announces the extended size, as the API requires.
    unsafe { GetMonitorInfoW(handle, ptr) }
        .as_bool()
        .then_some(info)
}

fn device_name(handle: HMONITOR) -> Option<String> {
    let info = monitor_info_ex(handle)?;
    let name = util::from_wide(&info.szDevice);
    (!name.is_empty()).then_some(name)
}

fn monitor_info(handle: HMONITOR) -> Option<MonitorInfo> {
    let info = monitor_info_ex(handle)?;
    let device = util::from_wide(&info.szDevice);
    if device.is_empty() {
        return None;
    }
    let dpi = effective_dpi(handle).unwrap_or_else(|e| {
        log::debug!("GetDpiForMonitor({device}) failed: {e}; assuming 96");
        BASE_DPI as u32
    });
    Some(MonitorInfo {
        id: MonitorId(device.clone()),
        name: device.trim_start_matches(r"\\.\").to_owned(),
        bounds: rect_from(info.monitorInfo.rcMonitor),
        work_area: rect_from(info.monitorInfo.rcWork),
        scale_factor: f64::from(dpi) / BASE_DPI,
        is_primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
    })
}

fn effective_dpi(handle: HMONITOR) -> windows::core::Result<u32> {
    let (mut x, mut y) = (0u32, 0u32);
    // SAFETY: both out-pointers address live locals for the duration of the call.
    unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut x, &mut y) }?;
    // X and Y are always equal for MDT_EFFECTIVE_DPI; prefer X, fall back to Y.
    Ok(if x != 0 { x } else { y })
}

fn rect_from(r: RECT) -> Rect {
    Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
}
