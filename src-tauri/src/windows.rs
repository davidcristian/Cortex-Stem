//! Window lifecycle: the dock window(s) and the settings window.
//!
//! One dock window per monitor (label `dock` for the primary, `dock-<id>`
//! for others; M3). The window is transparent, undecorated, always on top,
//! hidden from Alt+Tab and the stock taskbar, and does not take focus when
//! clicked: `focusable(false)` makes tao itself keep `WS_EX_NOACTIVATE`
//! across its style rewrites, and `Platform::prepare_dock_window` (called on
//! the main thread, right after creation) adds the rest. It never
//! closes: close requests are refused (this is a tray app; see `lib.rs`).
//!
//! Sizing (`docs/ipc/sizing-protocol.md`, v1.3): the window always spans the **full
//! width of the work area** and sits on its bottom edge, so the UI can
//! centre the panel and animate items in and out without any window resize.
//! The frontend reports its content size in CSS pixels through
//! `set_dock_content_size`; this module scales the *height* by the window's
//! scale factor and clamps it to the work area ([`plan_dock_rect`], pure and
//! unit-tested); the width is informational (logged; a lower bound only on
//! hosts without a monitor list). The last reported size is remembered
//! ([`DockGeometry`]) so the layout can be redone when the service reports a
//! monitor change. Clicks outside the panel fall through to the desktop via
//! the hit region ([`apply_hit_region`]), which `behavior.clickThrough`
//! switches off and on without a restart ([`on_settings_changed`]):
//! the UI's latest report is remembered while the setting is off and
//! replayed when it is turned on again.
//!
//! Auto-hide and the toggle: the UI reports when auto-hide slides
//! its panel out and back (`set_dock_auto_hidden`, kept in
//! [`DockAutoHidden`]), so that [`toggle_dock`] (tray click, Ctrl+Alt+D)
//! brings a slid-out panel back with one press (the `dock://show` event)
//! instead of hiding the window first.
//!
//! The dock panel: the UI reports where its panel rests
//! (`set_dock_panel_rect`, CSS px of the viewport); [`apply_dock_panel`]
//! remembers the rect and hands it to the service in physical pixels of the
//! virtual desktop ([`panel_on_screen`]: scaled like the hit region and
//! offset by the window's position), which is what the engine compares
//! window frames against. The conversion is redone whenever this module
//! moves the window ([`place`]: a monitor or work-area change moves it
//! without the UI's viewport changing), and the service gets `None` while
//! the window is hidden by the tray or the toggle shortcut ([`toggle_dock`]).
//!
//! The settings window (`settings`) is an ordinary decorated window,
//! created on demand and destroyed when closed. Its native title bar follows
//! the *Theme* setting (`settings.theme`): set when the window is
//! created and again whenever the setting changes ([`on_settings_changed`]).

use std::collections::HashMap;

use parking_lot::Mutex;
use stem_core::settings::ThemeMode;
use stem_core::{MonitorInfo, Rect, Settings};
use stem_platform::{DockWindowOptions, HitRegion, NativeWindowHandle, PlatformError};
use stem_service::DockService;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

pub const DOCK_LABEL: &str = "dock";
pub const SETTINGS_LABEL: &str = "settings";

/// Initial dock window size (logical px): the height stands until the UI
/// reports its content; the width is replaced by the work area's as soon as
/// the window is placed.
const INITIAL_SIZE: (f64, f64) = (640.0, 96.0);
/// What the UI may ask for, in CSS px (anything else is clamped).
const MIN_CONTENT: (f64, f64) = (64.0, 32.0);
// The height is generous on purpose: during a drag-to-reorder the UI asks
// for a very tall window so the lifted icon can go anywhere on screen; the
// work area clamps it further.
const MAX_CONTENT: (f64, f64) = (8192.0, 4096.0);

const SETTINGS_SIZE: (f64, f64) = (760.0, 600.0);
const SETTINGS_MIN_SIZE: (f64, f64) = (560.0, 420.0);

/// What the dock UI reported last (CSS px), kept in Tauri's managed state so
/// a monitor change can redo the layout and a settings change can apply the
/// hit region after the fact.
#[derive(Debug, Default)]
pub struct DockGeometry {
    content_css: Mutex<Option<(f64, f64)>>,
    /// `(panel, envelope)` of the latest `set_dock_hit_region`; applied
    /// while `behavior.clickThrough` is on and replayed when it is turned
    /// back on.
    hit_region_css: Mutex<Option<(Rect, Rect)>>,
    /// `behavior.clickThrough` as currently in effect, so a settings event
    /// only acts on a change.
    click_through: Mutex<Option<bool>>,
    /// The resting panel of the latest `set_dock_panel_rect`,
    /// converted again whenever the window moves or is shown.
    panel_css: Mutex<Option<Rect>>,
}

/// `settings.theme` as the settings window's title bar last took it (or
/// will take it, while the window is closed), so a settings event only acts
/// on a change. Kept in Tauri's managed state.
#[derive(Debug, Default)]
pub struct SettingsWindowTheme(Mutex<Option<ThemeMode>>);

/// Whether each dock window's panel is slid out by auto-hide, as
/// its UI last reported through `set_dock_auto_hidden`, keyed by window
/// label. `false` for a window that never reported, and again when the
/// window is created. Kept in Tauri's managed state so [`toggle_dock`] can
/// tell "bring the panel back" from "hide the window".
#[derive(Debug, Default)]
pub struct DockAutoHidden(Mutex<HashMap<String, bool>>);

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
        // tao derives WS_EX_NOACTIVATE from this flag, so the bit survives
        // its own style rewrites (`show`, `hide`, `set_always_on_top`, …).
        .focusable(false)
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

    // On the main thread (inside `setup`), which created the window: the
    // platform subclasses it and that is only allowed from its own thread.
    apply_native_styles(service, &window);
    let click_through = service.settings().behavior.click_through;
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        *geometry.click_through.lock() = Some(click_through);
    }
    // A fresh page starts with its panel shown.
    if let Some(flags) = app.try_state::<DockAutoHidden>() {
        flags.0.lock().insert(DOCK_LABEL.to_owned(), false);
    }
    log::info!(
        "click-through outside the panel: {}",
        describe_click_through(click_through)
    );
    position_window(&window, primary_monitor(&service.monitors()).as_ref())?;
    window.show()?;
    Ok(())
}

fn describe_click_through(enabled: bool) -> &'static str {
    if enabled {
        "on"
    } else {
        "off (the whole dock window takes clicks)"
    }
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

/// Remember the UI's hit region (CSS px, window-local) and, while
/// `behavior.clickThrough` is on, hand it to the platform in physical
/// pixels. While the setting is off the window stays fully clickable and
/// the report waits for the setting to be turned on ([`on_settings_changed`]).
/// A platform without the feature (mock, non-Windows hosts) leaves the
/// window fully clickable; that is logged once at debug and is not an error
/// for the UI. Runs on the main thread.
pub fn apply_hit_region(
    app: &AppHandle,
    window: &WebviewWindow,
    panel: Rect,
    envelope: Rect,
) -> Result<(), String> {
    let service = app
        .try_state::<crate::AppService>()
        .ok_or_else(|| "service not available".to_owned())?;
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        *geometry.hit_region_css.lock() = Some((panel, envelope));
    }
    if !service.0.settings().behavior.click_through {
        log::trace!(
            "hit region of {:?} remembered; click-through is off",
            window.label()
        );
        return Ok(());
    }
    push_hit_region(&service.0, window, panel, envelope)
}

/// The platform call behind [`apply_hit_region`]: CSS px → physical px by
/// the window's scale factor, then `set_dock_hit_region`. Main thread (the
/// scale factor is a window getter).
fn push_hit_region(
    service: &DockService,
    window: &WebviewWindow,
    panel: Rect,
    envelope: Rect,
) -> Result<(), String> {
    let Some(handle) = native_handle(window) else {
        log::debug!("hit region ignored: no native window handle on this host");
        return Ok(());
    };
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let region = HitRegion {
        panel: scale_rect(panel, scale),
        envelope: scale_rect(envelope, scale),
    };
    match service.platform().set_dock_hit_region(handle, Some(region)) {
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

/// Argument check for `set_dock_panel_rect` (`docs/ipc/dock-panel.md`):
/// non-negative sizes. Pure.
pub fn validate_panel_rect(rect: Rect) -> Result<(), String> {
    if rect.width < 0 || rect.height < 0 {
        return Err(format!("the panel has a negative size: {rect:?}"));
    }
    Ok(())
}

/// Remember the UI's resting panel (CSS px, viewport-relative) and hand it
/// to the service in physical screen pixels; `None` goes instead
/// while the window is hidden. Runs on the main thread.
pub fn apply_dock_panel(app: &AppHandle, window: &WebviewWindow, rect: Rect) -> Result<(), String> {
    let service = app
        .try_state::<crate::AppService>()
        .ok_or_else(|| "service not available".to_owned())?;
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        *geometry.panel_css.lock() = Some(rect);
    }
    push_dock_panel(&service.0, window, rect)
}

/// The service call behind [`apply_dock_panel`]: CSS px → physical screen
/// px from the window's position and scale factor, then
/// `DockService::set_dock_panel`; a hidden window reports no panel. Main
/// thread (the getters are window calls).
fn push_dock_panel(service: &DockService, window: &WebviewWindow, css: Rect) -> Result<(), String> {
    if !window.is_visible().map_err(|e| e.to_string())? {
        service.set_dock_panel(None);
        return Ok(());
    }
    let origin = window.outer_position().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let panel = panel_on_screen((origin.x, origin.y), css, scale);
    log::trace!(
        "dock panel of {:?}: {css:?} css at {scale}x from {origin:?} -> {panel:?}",
        window.label()
    );
    service.set_dock_panel(Some(panel));
    Ok(())
}

/// The window was moved by this module (`place`), or shown again: convert
/// the UI's latest panel rect for the window's new place. `origin` is the
/// window's top-left corner in physical pixels when known (just placed),
/// else it is read from the window. Main thread; silent when the UI has
/// reported nothing yet.
fn sync_dock_panel(window: &WebviewWindow, origin: Option<(i32, i32)>) {
    let app = window.app_handle();
    let (Some(service), Some(geometry)) = (
        app.try_state::<crate::AppService>(),
        app.try_state::<DockGeometry>(),
    ) else {
        return;
    };
    let Some(css) = *geometry.panel_css.lock() else {
        return;
    };
    let result = match origin {
        Some(origin) if window.is_visible().unwrap_or(false) => match window.scale_factor() {
            Ok(scale) => {
                service
                    .0
                    .set_dock_panel(Some(panel_on_screen(origin, css, scale)));
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        },
        _ => push_dock_panel(&service.0, window, css),
    };
    if let Err(e) = result {
        log::debug!("the dock panel could not be converted after a move: {e}");
    }
}

/// The resting panel in physical pixels of the virtual desktop: `css`
/// scaled like a hit region (`scale_rect`, rounded outwards) and offset by
/// the window's top-left corner `origin` (physical px). Pure.
pub fn panel_on_screen(origin: (i32, i32), css: Rect, scale: f64) -> Rect {
    let scaled = scale_rect(css, scale);
    Rect::new(
        origin.0.saturating_add(scaled.x),
        origin.1.saturating_add(scaled.y),
        scaled.width,
        scaled.height,
    )
}

/// The settings changed (settings window, tray, a command): apply
/// `behavior.clickThrough` and `theme` where they differ
/// from what is in effect. Runs on the service worker thread; the window
/// work hops to the main thread, where the window getters are direct calls.
pub fn on_settings_changed(app: &AppHandle, settings: &Settings) {
    on_click_through_setting(app, settings.behavior.click_through);
    on_theme_setting(app, settings.theme);
}

fn on_click_through_setting(app: &AppHandle, enabled: bool) {
    let Some(geometry) = app.try_state::<DockGeometry>() else {
        return;
    };
    {
        let mut current = geometry.click_through.lock();
        if *current == Some(enabled) {
            return;
        }
        *current = Some(enabled);
    }
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply_click_through(&handle, enabled)) {
        log::warn!("could not schedule the click-through change: {e}");
    }
}

/// Turn click-through on (replay the UI's latest hit region) or off (the
/// whole dock window takes clicks again). Main thread.
fn apply_click_through(app: &AppHandle, enabled: bool) {
    let Some(window) = app.get_webview_window(DOCK_LABEL) else {
        log::debug!("click-through changed before the dock window exists");
        return;
    };
    let Some(service) = app.try_state::<crate::AppService>() else {
        return;
    };
    if enabled {
        let region = app
            .try_state::<DockGeometry>()
            .and_then(|geometry| *geometry.hit_region_css.lock());
        match region {
            Some((panel, envelope)) => {
                match push_hit_region(&service.0, &window, panel, envelope) {
                    Ok(()) => log::info!(
                        "click-through outside the panel: {}",
                        describe_click_through(true)
                    ),
                    Err(e) => log::warn!("click-through could not be turned on: {e}"),
                }
            }
            None => {
                log::info!("click-through outside the panel: on (waiting for the UI's hit region)")
            }
        }
        return;
    }
    let Some(handle) = native_handle(&window) else {
        return;
    };
    match service.0.platform().set_dock_hit_region(handle, None) {
        Ok(()) | Err(PlatformError::Unsupported(_)) => log::info!(
            "click-through outside the panel: {}",
            describe_click_through(false)
        ),
        Err(e) => log::warn!("click-through could not be turned off: {e}"),
    }
}

/// `settings.theme` changed: the settings window's title bar follows it (the
/// page itself follows `dock://settings`). Any thread; never blocks.
fn on_theme_setting(app: &AppHandle, mode: ThemeMode) {
    let Some(state) = app.try_state::<SettingsWindowTheme>() else {
        return;
    };
    {
        let mut current = state.0.lock();
        if *current == Some(mode) {
            return;
        }
        *current = Some(mode);
    }
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply_settings_window_theme(&handle, mode)) {
        log::warn!("could not schedule the settings window's theme change: {e}");
    }
}

/// Give the open settings window's title bar the theme for `mode`. A closed
/// window takes the setting when it is created. Main thread.
fn apply_settings_window_theme(app: &AppHandle, mode: ThemeMode) {
    let Some(window) = app.get_webview_window(SETTINGS_LABEL) else {
        log::debug!("theme setting {mode:?}: the settings window is not open");
        return;
    };
    match window.set_theme(native_theme(mode)) {
        Ok(()) => log::debug!("settings window title bar: {mode:?}"),
        Err(e) => log::warn!("the settings window's theme could not be set: {e}"),
    }
}

/// `settings.theme` → the native window theme: forced light or dark, or
/// `None` to follow Windows. Pure.
pub fn native_theme(mode: ThemeMode) -> Option<tauri::Theme> {
    match mode {
        ThemeMode::System => None,
        ThemeMode::Light => Some(tauri::Theme::Light),
        ThemeMode::Dark => Some(tauri::Theme::Dark),
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

/// Size and place the window for a content size in CSS px: the work area's
/// full width, the content height.
fn layout(
    window: &WebviewWindow,
    monitors: &[MonitorInfo],
    width: f64,
    height: f64,
) -> tauri::Result<()> {
    let monitor = primary_monitor(monitors);
    let mut work_area = match &monitor {
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
    if monitor.is_none() {
        // No monitor list to trust: the content width is the lower bound,
        // so the panel is never cut (`docs/ipc/sizing-protocol.md`).
        work_area.width = work_area.width.max(physical_px(width, scale));
    }
    let rect = plan_dock_rect(work_area, scale, height);
    log::debug!(
        "dock window: {width}x{height} css at {scale}x -> {rect:?} in {work_area:?} (full width)"
    );
    place(window, rect)
}

/// Span the work area's width at the window's current height, on the work
/// area's bottom edge. Used before the UI has reported a content size.
fn position_window(window: &WebviewWindow, monitor: Option<&MonitorInfo>) -> tauri::Result<()> {
    let size: PhysicalSize<u32> = window.outer_size()?;
    let work_area = match monitor {
        Some(m) => m.work_area,
        None => fallback_work_area(window)?,
    };
    let height = i32::try_from(size.height).unwrap_or(i32::MAX);
    place(window, full_width_rect(work_area, height))
}

/// Size and position the dock window, then redo the panel conversion for
/// its new place.
fn place(window: &WebviewWindow, rect: Rect) -> tauri::Result<()> {
    window.set_size(PhysicalSize::new(
        rect.width.unsigned_abs(),
        rect.height.unsigned_abs(),
    ))?;
    window.set_position(PhysicalPosition::new(rect.x, rect.y))?;
    sync_dock_panel(window, Some((rect.x, rect.y)));
    Ok(())
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

/// Physical window rectangle for a content height in CSS px: the work
/// area's full width; the height scaled by the window's scale factor and
/// clamped to the work area; flush with the work area's bottom edge. Pure.
pub fn plan_dock_rect(work_area: Rect, scale: f64, css_height: f64) -> Rect {
    full_width_rect(work_area, physical_px(css_height, scale))
}

/// The rectangle spanning the full width of `work_area`, `height` physical
/// pixels high (clamped to the work area, never empty), on its bottom edge.
/// Pure placement rule, unit-tested below.
pub fn full_width_rect(work_area: Rect, height: i32) -> Rect {
    let width = work_area.width.max(1);
    let height = height.clamp(1, work_area.height.max(1));
    Rect::new(work_area.x, work_area.bottom() - height, width, height)
}

/// CSS px → physical px, rounded to the nearest pixel (never negative).
fn physical_px(css: f64, scale: f64) -> i32 {
    ((css * scale).round().max(0.0)).min(f64::from(i32::MAX)) as i32
}

// ---------------------------------------------------------------------------
// Settings window
// ---------------------------------------------------------------------------

/// Open the settings window, or show and focus it if it already exists.
/// Closing it destroys it (Tauri's default); the app keeps running. A new
/// window's title bar takes the current *Theme* setting. Main thread.
pub fn open_settings_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        window.show()?;
        window.set_focus()?;
        log::debug!("settings window focused");
        return Ok(());
    }
    let mode = app
        .try_state::<crate::AppService>()
        .map(|service| service.0.settings().theme);
    if let (Some(mode), Some(state)) = (mode, app.try_state::<SettingsWindowTheme>()) {
        *state.0.lock() = Some(mode);
    }
    WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings.html".into()))
        .title("Stem Settings")
        .theme(mode.and_then(native_theme))
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
// Visibility helpers (tray, global shortcut, single instance)
// ---------------------------------------------------------------------------

/// The dock UI of `window` reports that auto-hide slid its panel out
/// (`hidden`), or brought it back (`docs/ipc/toggle-while-hidden.md`).
/// Remembered per window label for [`toggle_dock`]; logged on a
/// change.
pub fn set_auto_hidden(
    app: &AppHandle,
    window: &WebviewWindow,
    hidden: bool,
) -> Result<(), String> {
    let flags = app
        .try_state::<DockAutoHidden>()
        .ok_or_else(|| "auto-hide state not available".to_owned())?;
    let previous = flags.0.lock().insert(window.label().to_owned(), hidden);
    if previous != Some(hidden) {
        log::debug!(
            "dock panel of {:?} {}",
            window.label(),
            if hidden { "auto-hidden" } else { "shown" }
        );
    }
    Ok(())
}

/// What the UI of the dock window `label` last reported through
/// [`set_auto_hidden`]; `false` until it reports.
fn is_auto_hidden(app: &AppHandle, label: &str) -> bool {
    app.try_state::<DockAutoHidden>()
        .is_some_and(|flags| flags.0.lock().get(label).copied().unwrap_or(false))
}

/// Tray left click and the global toggle shortcut: one press brings
/// the dock back whatever hid it, the next hides it.
/// - Window hidden: show it; the engine gets its panel back.
/// - Window visible but its panel slid out by auto-hide
///   ([`DockAutoHidden`]): ask the UI to bring the panel back
///   (`dock://show`); the window stays as it is.
/// - Window visible and its panel shown: hide the window; the engine has no
///   panel on screen meanwhile.
///
/// The UI reports its phase after the fact, so a press that lands while the
/// panel is sliding out may still hide the window; the next press shows it
/// (and the page, visible again, brings the panel back).
pub fn toggle_dock(app: &AppHandle) {
    let Some(window) = app.get_webview_window(DOCK_LABEL) else {
        log::warn!("toggle_dock: no dock window");
        return;
    };
    let result = match window.is_visible() {
        Ok(true) if is_auto_hidden(app, window.label()) => {
            log::debug!("dock reveal requested");
            app.emit_to(window.label(), crate::events::SHOW, ())
        }
        Ok(true) => window.hide().inspect(|()| {
            log::debug!("dock window hidden");
            if let Some(service) = app.try_state::<crate::AppService>() {
                service.0.set_dock_panel(None);
            }
        }),
        Ok(false) => window.show().inspect(|()| {
            log::debug!("dock window shown");
            sync_dock_panel(&window, None);
        }),
        Err(e) => Err(e),
    };
    if let Err(e) = result {
        log::warn!("toggle_dock failed: {e}");
    }
}

/// Show and focus every window we have (the dock, and the settings window
/// when it is open). Used when a second instance is started. A dock window
/// that is on screen with its panel slid out by auto-hide is asked to bring
/// the panel back instead (`dock://show`, as [`toggle_dock`] does).
pub fn reveal_all(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(DOCK_LABEL)
            && window.is_visible().unwrap_or(false)
            && is_auto_hidden(app, &label)
        {
            log::debug!("dock reveal requested by a second instance");
            if let Err(e) = app.emit_to(&label, crate::events::SHOW, ()) {
                log::warn!("reveal {label}: {e}");
            }
            continue;
        }
        if let Err(e) = window.show().and_then(|()| window.set_focus()) {
            log::warn!("reveal {label}: {e}");
        } else if label.starts_with(DOCK_LABEL) {
            sync_dock_panel(&window, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_width_rect_spans_the_work_area_on_its_bottom_edge() {
        let wa = Rect::new(100, 50, 1000, 900);
        assert_eq!(full_width_rect(wa, 80), Rect::new(100, 870, 1000, 80));
        // A secondary monitor left of the primary: negative x is kept.
        let left = Rect::new(-1920, 0, 1920, 1040);
        assert_eq!(full_width_rect(left, 96), Rect::new(-1920, 944, 1920, 96));
        // Taller than the work area: clamped to it.
        assert_eq!(full_width_rect(wa, 5000), Rect::new(100, 50, 1000, 900));
        // Degenerate heights never produce an empty window.
        assert_eq!(full_width_rect(wa, 0).height, 1);
        assert_eq!(full_width_rect(wa, -5).height, 1);
        assert_eq!(
            full_width_rect(Rect::new(0, 0, 0, 0), 10),
            Rect::new(0, -1, 1, 1)
        );
    }

    #[test]
    fn plan_dock_rect_scales_the_css_height_and_keeps_the_full_width() {
        let wa = Rect::new(0, 0, 2560, 1400);
        // 150 % scaling: 96 css -> 144 physical, the work area's width.
        assert_eq!(plan_dock_rect(wa, 1.5, 96.0), Rect::new(0, 1256, 2560, 144));
        // Fractional results round to the nearest pixel.
        assert_eq!(plan_dock_rect(wa, 1.25, 97.0).height, 121);
        assert_eq!(physical_px(641.0, 1.25), 801);
        assert_eq!(physical_px(-3.0, 1.0), 0);
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
    fn panel_on_screen_scales_like_a_hit_region_and_offsets_by_the_window_origin() {
        // A 561 × 64 panel at (360, 168) in a window placed at (0, 1248) at 1×.
        assert_eq!(
            panel_on_screen((0, 1248), Rect::new(360, 168, 561, 64), 1.0),
            Rect::new(360, 1416, 561, 64)
        );
        // 150 %: the CSS rect rounds outwards before the offset (11 css → 16.5 → 16).
        assert_eq!(
            panel_on_screen((100, 2000), Rect::new(11, 0, 10, 8), 1.5),
            Rect::new(116, 2000, 16, 12)
        );
        // A monitor left of the primary: negative origin, kept.
        assert_eq!(
            panel_on_screen((-1920, 944), Rect::new(600, 20, 500, 70), 1.0),
            Rect::new(-1320, 964, 500, 70)
        );
        assert_eq!(validate_panel_rect(Rect::new(0, 0, 0, 0)), Ok(()));
        assert!(validate_panel_rect(Rect::new(0, 0, -1, 10)).is_err());
        assert!(validate_panel_rect(Rect::new(0, 0, 10, -1)).is_err());
    }

    #[test]
    fn native_theme_forces_light_or_dark_and_follows_windows_for_system() {
        assert_eq!(native_theme(ThemeMode::System), None);
        assert_eq!(native_theme(ThemeMode::Light), Some(tauri::Theme::Light));
        assert_eq!(native_theme(ThemeMode::Dark), Some(tauri::Theme::Dark));
    }

    #[test]
    fn plan_dock_rect_clamps_to_the_work_area() {
        let wa = Rect::new(-1920, 0, 1920, 1040);
        let rect = plan_dock_rect(wa, 2.0, 1024.0);
        assert_eq!(rect, Rect::new(-1920, 0, 1920, 1040));
        // Degenerate sizes never produce a zero-sized window.
        assert_eq!(plan_dock_rect(wa, 1.0, 0.0).height, 1);
        assert_eq!(plan_dock_rect(Rect::new(0, 0, 0, 0), 1.0, 10.0).height, 1);
        assert_eq!(plan_dock_rect(Rect::new(0, 0, 0, 0), 1.0, 10.0).width, 1);
    }
}
