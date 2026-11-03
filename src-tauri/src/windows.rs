//! Window lifecycle: the dock window(s) and the settings window.
//!
//! One dock window per monitor (label `dock` for the primary, `dock-<id>`
//! for others; M3). The window is transparent, undecorated, always on top,
//! hidden from Alt+Tab and the stock taskbar, and does not take focus when
//! clicked (`Platform::prepare_dock_window`). It never closes: close requests
//! are refused (this is a tray app; see `lib.rs`).
//!
//! Sizing (`docs/ipc/sizing-protocol.md`): the frontend owns the layout and reports
//! its content size in CSS pixels through `set_dock_content_size`. This
//! module converts it to physical pixels with the window's scale factor,
//! clamps it to the monitor's work area and places the window bottom-centred
//! on that work area ([`plan_dock_rect`], pure and unit-tested). The last
//! reported size is remembered ([`DockGeometry`]) so the layout can be redone
//! when the service reports a monitor change.
//!
//! The settings window (`settings`) is an ordinary decorated window,
//! created on demand and destroyed when closed.

use parking_lot::Mutex;
use stem_core::{MonitorInfo, Rect};
use stem_platform::{DockWindowOptions, HitRegion, NativeWindowHandle, PlatformError};
use stem_service::DockService;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

pub const DOCK_LABEL: &str = "dock";
pub const SETTINGS_LABEL: &str = "settings";

/// Initial dock window size (logical px) before the UI reports its content.
const INITIAL_SIZE: (f64, f64) = (640.0, 96.0);
/// What the UI may ask for, in CSS px (anything else is clamped).
const MIN_CONTENT: (f64, f64) = (64.0, 32.0);
const MAX_CONTENT: (f64, f64) = (8192.0, 1024.0);

const SETTINGS_SIZE: (f64, f64) = (760.0, 600.0);
const SETTINGS_MIN_SIZE: (f64, f64) = (560.0, 420.0);

/// The last content size the dock UI reported (CSS px), kept in Tauri's
/// managed state so a monitor change can redo the layout.
#[derive(Debug, Default)]
pub struct DockGeometry {
    content_css: Mutex<Option<(f64, f64)>>,
}

// ---------------------------------------------------------------------------
// dock window
// ---------------------------------------------------------------------------

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

    // The dock is never closed by the user; Quit lives in the tray.
    window.on_window_event(|event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            log::debug!("dock window close request ignored");
            api.prevent_close();
        }
    });

    apply_native_styles(service, &window);
    position_window(&window, primary_monitor(&service.monitors()).as_ref())?;
    window.show()?;
    Ok(())
}

/// The OS handle of a Tauri window, for `Platform` calls. `None` where the
/// platform has no native handles (non-Windows hosts) or the lookup fails.
pub fn native_handle(window: &WebviewWindow) -> Option<NativeWindowHandle> {
    #[cfg(windows)]
    {
        match window.hwnd() {
            Ok(hwnd) => Some(NativeWindowHandle(hwnd.0 as isize)),
            Err(e) => {
                log::warn!("no HWND for {:?}: {e}", window.label());
                None
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
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

/// Resize to the reported content size (CSS px) and re-centre on the primary
/// monitor's work area. Called from the `set_dock_content_size` command, on
/// the main thread.
pub fn apply_content_size(
    app: &AppHandle,
    window: &WebviewWindow,
    width: f64,
    height: f64,
) -> tauri::Result<()> {
    let width = width.clamp(MIN_CONTENT.0, MAX_CONTENT.0);
    let height = height.clamp(MIN_CONTENT.1, MAX_CONTENT.1);
    match app.try_state::<DockGeometry>() {
        Some(geometry) => *geometry.content_css.lock() = Some((width, height)),
        None => log::warn!("DockGeometry state missing; monitor changes will not re-layout"),
    }
    let monitors = match app.try_state::<crate::AppService>() {
        Some(service) => service.0.monitors(),
        None => Vec::new(),
    };
    layout(window, &monitors, width, height)
}

/// Argument check for `set_dock_hit_region` (`docs/ipc/hit-region.md`):
/// non-negative sizes, and the envelope contains the panel. Pure.
pub fn validate_hit_region(panel: Rect, envelope: Rect) -> Result<(), String> {
    for (name, rect) in [("panel", panel), ("envelope", envelope)] {
        if rect.width < 0 || rect.height < 0 {
            return Err(format!("{name} has a negative size: {rect:?}"));
        }
    }
    let contains = envelope.x <= panel.x
        && envelope.y <= panel.y
        && envelope.right() >= panel.right()
        && envelope.bottom() >= panel.bottom();
    if !contains {
        return Err(format!(
            "envelope {envelope:?} does not contain panel {panel:?}"
        ));
    }
    Ok(())
}

/// Hand the UI's hit region (CSS px, window-local) to the platform in
/// physical pixels. A platform without the feature (mock, non-Windows
/// hosts) leaves the window fully clickable; that is logged once at debug
/// and is not an error for the UI. Runs on the main thread.
pub fn apply_hit_region(
    app: &AppHandle,
    window: &WebviewWindow,
    panel: Rect,
    envelope: Rect,
) -> Result<(), String> {
    let service = app
        .try_state::<crate::AppService>()
        .ok_or_else(|| "service not available".to_owned())?;
    let Some(handle) = native_handle(window) else {
        log::debug!("hit region ignored: no native window handle on this host");
        return Ok(());
    };
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let region = HitRegion {
        panel: scale_rect(panel, scale),
        envelope: scale_rect(envelope, scale),
    };
    match service
        .0
        .platform()
        .set_dock_hit_region(handle, Some(region))
    {
        Ok(()) => {
            log::trace!("hit region of {:?}: {region:?}", window.label());
            Ok(())
        }
        Err(PlatformError::Unsupported(task)) => {
            static LOGGED: std::sync::Once = std::sync::Once::new();
            LOGGED.call_once(|| {
                log::debug!("hit regions are not supported by this platform (feature: {task})");
            });
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// CSS px → physical px, rounded outwards (left/top down, right/bottom up)
/// so a region never loses an edge pixel to rounding. Pure.
pub fn scale_rect(rect: Rect, scale: f64) -> Rect {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let left = (f64::from(rect.x) * scale).floor();
    let top = (f64::from(rect.y) * scale).floor();
    let right = (f64::from(rect.right()) * scale).ceil();
    let bottom = (f64::from(rect.bottom()) * scale).ceil();
    let clamp = |v: f64| v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    Rect::new(
        clamp(left),
        clamp(top),
        clamp(right - left),
        clamp(bottom - top),
    )
}

/// The service reported a new monitor list (startup, hot-plug, resolution
/// or taskbar change). Runs on the service worker thread; the window work
/// hops to the main thread so the blocking getters run there directly.
pub fn on_monitors_changed(app: &AppHandle, monitors: Vec<MonitorInfo>) {
    let handle = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(DOCK_LABEL) else {
            log::debug!("monitors changed before the dock window exists");
            return;
        };
        let content = handle
            .try_state::<DockGeometry>()
            .and_then(|g| *g.content_css.lock());
        let result = match content {
            Some((width, height)) => layout(&window, &monitors, width, height),
            None => position_window(&window, primary_monitor(&monitors).as_ref()),
        };
        if let Err(e) = result {
            log::warn!("re-positioning the dock after a monitor change failed: {e}");
        }
    });
    if let Err(e) = scheduled {
        log::warn!("could not schedule the dock re-layout: {e}");
    }
}

/// Size and place the window for a content size in CSS px.
fn layout(
    window: &WebviewWindow,
    monitors: &[MonitorInfo],
    width: f64,
    height: f64,
) -> tauri::Result<()> {
    let monitor = primary_monitor(monitors);
    let work_area = match &monitor {
        Some(m) => m.work_area,
        None => fallback_work_area(window)?,
    };
    let scale = match window.scale_factor() {
        Ok(s) if s.is_finite() && s > 0.0 => s,
        Ok(s) => {
            log::warn!("window scale factor {s} is unusable; using the monitor's");
            monitor.as_ref().map_or(1.0, |m| m.scale_factor)
        }
        Err(e) => {
            log::warn!("window scale factor unavailable ({e}); using the monitor's");
            monitor.as_ref().map_or(1.0, |m| m.scale_factor)
        }
    };
    let rect = plan_dock_rect(work_area, scale, width, height);
    log::debug!("dock window: {width}x{height} css at {scale}x -> {rect:?} in {work_area:?}");
    window.set_size(PhysicalSize::new(
        rect.width.unsigned_abs(),
        rect.height.unsigned_abs(),
    ))?;
    window.set_position(PhysicalPosition::new(rect.x, rect.y))
}

/// Centre the window, at its current size, at the bottom of the work area.
/// Used before the UI has reported a content size.
fn position_window(window: &WebviewWindow, monitor: Option<&MonitorInfo>) -> tauri::Result<()> {
    let size: PhysicalSize<u32> = window.outer_size()?;
    let work_area = match monitor {
        Some(m) => m.work_area,
        None => fallback_work_area(window)?,
    };
    let (x, y) = bottom_center(work_area, size.width as i32, size.height as i32);
    window.set_position(PhysicalPosition::new(x, y))
}

/// Before the platform reported monitors, fall back to Tauri's own view: the
/// window's monitor, else the primary one (only queried when needed, so its
/// failure cannot mask a successful first answer).
fn fallback_work_area(window: &WebviewWindow) -> tauri::Result<Rect> {
    let monitor = match window.current_monitor()? {
        Some(current) => Some(current),
        None => window.primary_monitor()?,
    };
    Ok(match monitor {
        Some(m) => {
            let pos = m.work_area().position;
            let size = m.work_area().size;
            Rect::new(pos.x, pos.y, size.width as i32, size.height as i32)
        }
        None => Rect::new(0, 0, 1920, 1040),
    })
}

/// Physical window rectangle for a content size in CSS px: scaled by the
/// window's scale factor, clamped to the work area, bottom-centred. Pure.
pub fn plan_dock_rect(work_area: Rect, scale: f64, css_width: f64, css_height: f64) -> Rect {
    let physical = |css: f64, max: i32| ((css * scale).round() as i32).clamp(1, max.max(1));
    let width = physical(css_width, work_area.width);
    let height = physical(css_height, work_area.height);
    let (x, y) = bottom_center(work_area, width, height);
    Rect::new(x, y, width, height)
}

/// Pure placement rule, unit-tested below.
pub fn bottom_center(work_area: Rect, width: i32, height: i32) -> (i32, i32) {
    let x = work_area.x + (work_area.width - width) / 2;
    let y = work_area.bottom() - height;
    (x, y)
}

// ---------------------------------------------------------------------------
// Settings window
// ---------------------------------------------------------------------------

/// Open the settings window, or show and focus it if it already exists.
/// Closing it destroys it (Tauri's default); the app keeps running.
pub fn open_settings_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        window.show()?;
        window.set_focus()?;
        log::debug!("settings window focused");
        return Ok(());
    }
    WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings.html".into()))
        .title("Stem Settings")
        .inner_size(SETTINGS_SIZE.0, SETTINGS_SIZE.1)
        .min_inner_size(SETTINGS_MIN_SIZE.0, SETTINGS_MIN_SIZE.1)
        .resizable(true)
        .decorations(true)
        .always_on_top(false)
        .center()
        .focused(true)
        .build()?;
    log::info!("settings window opened");
    Ok(())
}

// ---------------------------------------------------------------------------
// Visibility helpers (tray, single instance)
// ---------------------------------------------------------------------------

/// Tray left click: hide the dock if it is visible, show it otherwise.
pub fn toggle_dock(app: &AppHandle) {
    let Some(window) = app.get_webview_window(DOCK_LABEL) else {
        log::warn!("toggle_dock: no dock window");
        return;
    };
    let result = match window.is_visible() {
        Ok(true) => window.hide(),
        Ok(false) => window.show(),
        Err(e) => Err(e),
    };
    match result {
        Ok(()) => log::debug!("dock visibility toggled"),
        Err(e) => log::warn!("toggle_dock failed: {e}"),
    }
}

/// Show and focus every window we have (the dock, and the settings window
/// when it is open). Used when a second instance is started.
pub fn reveal_all(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if let Err(e) = window.show().and_then(|()| window.set_focus()) {
            log::warn!("reveal {label}: {e}");
        }
    }
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

    #[test]
    fn plan_dock_rect_scales_css_to_physical_and_centres() {
        let wa = Rect::new(0, 0, 2560, 1400);
        // 150 % scaling: 640x96 css -> 960x144 physical, centred at the bottom.
        assert_eq!(
            plan_dock_rect(wa, 1.5, 640.0, 96.0),
            Rect::new(800, 1256, 960, 144)
        );
        // Fractional results round to the nearest pixel.
        assert_eq!(plan_dock_rect(wa, 1.25, 641.0, 97.0).width, 801);
    }

    #[test]
    fn scale_rect_rounds_outwards() {
        // 150 %: 10..=20 css → 15..=30 physical exactly.
        assert_eq!(
            scale_rect(Rect::new(10, 10, 10, 10), 1.5),
            Rect::new(15, 15, 15, 15)
        );
        // 125 %: left 11*1.25=13.75 → 13; right 21*1.25=26.25 → 27.
        assert_eq!(
            scale_rect(Rect::new(11, 0, 10, 8), 1.25),
            Rect::new(13, 0, 14, 10)
        );
        // A bad scale factor means "as is".
        assert_eq!(
            scale_rect(Rect::new(1, 2, 3, 4), f64::NAN),
            Rect::new(1, 2, 3, 4)
        );
        assert_eq!(
            scale_rect(Rect::new(1, 2, 3, 4), 0.0),
            Rect::new(1, 2, 3, 4)
        );
    }

    #[test]
    fn validate_hit_region_requires_containment_and_sizes() {
        let panel = Rect::new(100, 40, 400, 60);
        assert_eq!(
            validate_hit_region(panel, Rect::new(80, 0, 440, 100)),
            Ok(())
        );
        assert_eq!(validate_hit_region(panel, panel), Ok(()));
        assert!(validate_hit_region(panel, Rect::new(120, 0, 440, 100)).is_err());
        assert!(validate_hit_region(panel, Rect::new(80, 0, 440, 90)).is_err());
        assert!(validate_hit_region(Rect::new(0, 0, -1, 10), Rect::new(0, 0, 10, 10)).is_err());
        assert!(validate_hit_region(Rect::new(0, 0, 1, 1), Rect::new(0, 0, 10, -10)).is_err());
    }

    #[test]
    fn plan_dock_rect_clamps_to_the_work_area() {
        let wa = Rect::new(-1920, 0, 1920, 1040);
        let rect = plan_dock_rect(wa, 2.0, 8192.0, 1024.0);
        assert_eq!(rect, Rect::new(-1920, 0, 1920, 1040));
        // Degenerate sizes never produce a zero-sized window.
        assert_eq!(plan_dock_rect(wa, 1.0, 0.0, 0.0).width, 1);
        assert_eq!(
            plan_dock_rect(Rect::new(0, 0, 0, 0), 1.0, 10.0, 10.0).height,
            1
        );
    }
}
