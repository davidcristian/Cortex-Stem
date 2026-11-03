//! dock window lifecycle: creation, OS styling, sizing and positioning.
//!
//! One dock window per monitor (label `dock` for the primary, `dock-<id>`
//! for others; M3). The window is transparent, undecorated, always on top,
//! hidden from Alt+Tab and the stock taskbar, and does not take focus when
//! clicked (`Platform::prepare_dock_window`). The frontend owns the layout and
//! reports its content size through `set_dock_content_size`; this module turns
//! that into a window rectangle centred at the bottom of the monitor's work
//! area, with headroom for magnification.

use stem_core::{MonitorInfo, Rect};
use stem_platform::{DockWindowOptions, NativeWindowHandle};
use stem_service::DockService;
use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const DOCK_LABEL: &str = "dock";

/// Initial window size before the UI reports its real content size.
const INITIAL_SIZE: (f64, f64) = (640.0, 96.0);

pub fn create_dock_windows(app: &AppHandle, service: &DockService) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, DOCK_LABEL, WebviewUrl::App("dock.html".into()))
        .title("Stem")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .inner_size(INITIAL_SIZE.0, INITIAL_SIZE.1)
        .build()?;

    apply_native_styles(service, &window);
    position_window(&window, primary_monitor(&service.monitors()).as_ref())?;
    window.show()?;
    Ok(())
}

fn apply_native_styles(service: &DockService, window: &WebviewWindow) {
    #[cfg(windows)]
    {
        match window.hwnd() {
            Ok(hwnd) => {
                let handle = NativeWindowHandle(hwnd.0 as isize);
                if let Err(e) = service
                    .platform()
                    .prepare_dock_window(handle, DockWindowOptions::default())
                {
                    log::warn!("prepare_dock_window failed: {e}");
                }
            }
            Err(e) => log::warn!("no HWND for dock window: {e}"),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (
            service,
            window,
            NativeWindowHandle(0),
            DockWindowOptions::default(),
        );
    }
}

fn primary_monitor(monitors: &[MonitorInfo]) -> Option<MonitorInfo> {
    monitors
        .iter()
        .find(|m| m.is_primary)
        .or_else(|| monitors.first())
        .cloned()
}

/// Resize to the reported content size and re-centre on the monitor.
pub fn apply_content_size(
    app: &AppHandle,
    window: &WebviewWindow,
    width: f64,
    height: f64,
) -> tauri::Result<()> {
    let width = width.clamp(64.0, 8192.0);
    let height = height.clamp(32.0, 1024.0);
    window.set_size(LogicalSize::new(width, height))?;
    let service = app.state::<crate::AppService>();
    position_window(window, primary_monitor(&service.0.monitors()).as_ref())
}

/// Centre horizontally at the bottom of the monitor's work area.
fn position_window(window: &WebviewWindow, monitor: Option<&MonitorInfo>) -> tauri::Result<()> {
    let size: PhysicalSize<u32> = window.outer_size()?;
    let work_area = match monitor {
        Some(m) => m.work_area,
        None => fallback_work_area(window)?,
    };
    let (x, y) = bottom_center(work_area, size.width as i32, size.height as i32);
    window.set_position(PhysicalPosition::new(x, y))
}

/// Before the platform reported monitors, fall back to Tauri's own view.
fn fallback_work_area(window: &WebviewWindow) -> tauri::Result<Rect> {
    let monitor = window.current_monitor()?.or(window.primary_monitor()?);
    Ok(match monitor {
        Some(m) => {
            let pos = m.work_area().position;
            let size = m.work_area().size;
            Rect::new(pos.x, pos.y, size.width as i32, size.height as i32)
        }
        None => Rect::new(0, 0, 1920, 1040),
    })
}

/// Pure placement rule, unit-tested below.
pub fn bottom_center(work_area: Rect, width: i32, height: i32) -> (i32, i32) {
    let x = work_area.x + (work_area.width - width) / 2;
    let y = work_area.bottom() - height;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_center_places_window_centred_on_the_bottom_edge() {
        let wa = Rect::new(100, 50, 1000, 900);
        assert_eq!(bottom_center(wa, 400, 80), (400, 870));
        // Wider than the work area: still centred (negative x is allowed).
        assert_eq!(bottom_center(wa, 1200, 80), (0, 870));
    }
}
