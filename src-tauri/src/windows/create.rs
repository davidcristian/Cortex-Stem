//! Creating one dock window and preparing it for the platform.

use stem_core::MonitorInfo;
use stem_platform::{DockWindowOptions, NativeWindowHandle};
use stem_service::DockService;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use super::DOCK_LABEL;
use super::focus_release::on_dock_blurred;
use super::layout::layout;
use super::plan::{describe_monitor, dock_page};
use super::reconcile::on_dock_destroyed;
use super::relayout::{dock_edge, on_scale_factor_changed};
use super::state::{DockGeometry, DockWindowState};
use super::visibility::DockAutoHidden;

/// Initial dock window size (logical px) at creation: its height is the
/// thickness the window takes on whichever edge it is first placed
/// ([`thickness_of`] with no edge yet) and keeps until the UI reports its
/// content; the length is replaced by the work area's as soon as the window
/// is placed.
///
/// [`thickness_of`]: super::placement::thickness_of
pub(super) const INITIAL_SIZE: (f64, f64) = (640.0, 96.0);

/// Create one dock window on `monitor` and show it, unless the set is
/// hidden ([`show_new_dock_window`]). Main thread (the platform subclasses
/// the window right after creation, which is only allowed from the thread
/// that created it).
pub(super) fn create_dock_window(
    app: &AppHandle,
    service: &DockService,
    label: &str,
    monitor: Option<&MonitorInfo>,
) -> tauri::Result<()> {
    let page = dock_page(label, monitor.map(|m| &m.id));
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App(page.into()))
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

    // The dock is never closed by the user (Quit lives in the tray), a DPI
    // change lays it out again, losing the focus may end the focus
    // shortcut's session, and a window `close_dock_window` destroyed
    // is only gone once its `Destroyed` arrives.
    let handle = app.clone();
    let owned = label.to_owned();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            log::debug!("dock window {owned:?}: close request ignored");
            api.prevent_close();
        }
        WindowEvent::ScaleFactorChanged {
            scale_factor,
            new_inner_size,
            ..
        } => on_scale_factor_changed(&handle, &owned, *scale_factor, *new_inner_size),
        // The dock lost the keyboard focus; maybe the activation.
        WindowEvent::Focused(false) => on_dock_blurred(&handle, &owned),
        WindowEvent::Destroyed => on_dock_destroyed(&handle, &owned),
        _ => {}
    });

    apply_native_styles(service, &window);
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        geometry.docks.lock().insert(
            label.to_owned(),
            DockWindowState {
                monitor: monitor.map(|m| m.id.clone()),
                scale: window.scale_factor().ok(),
                ..DockWindowState::default()
            },
        );
    }
    // A fresh page starts with its panel shown.
    if let Some(flags) = app.try_state::<DockAutoHidden>() {
        flags.0.lock().insert(label.to_owned(), false);
    }
    // Not placed yet: the initial height becomes the thickness on any edge.
    layout(&window, monitor, dock_edge(app), None)?;
    let primary_visible = if label == DOCK_LABEL {
        None
    } else {
        primary_visibility(app)
    };
    if show_new_dock_window(label, primary_visible) {
        window.show()?;
        log::info!(
            "dock window created {label:?} on {}",
            describe_monitor(monitor)
        );
    } else {
        // Hidden, it reports no panel to the engine (`push_dock_panel`);
        // the next Show of the set (`toggle_dock`, which acts on every dock
        // window) shows it and hands its panel over.
        log::info!(
            "dock window created {label:?} on {}, hidden like the rest of the set",
            describe_monitor(monitor)
        );
    }
    Ok(())
}

/// Whether a dock window that was just created is shown at once.
/// The primary's window (`dock`) always is. Any other follows the set,
/// whose state is the primary's (as for [`toggle_dock`]): it is shown only
/// when `primary_visible` is `Some(true)`; a hidden primary means the toggle
/// hid the set, and a window shown then would be the one dock on screen,
/// turning the next press into a Show. `None` (the primary's state could
/// not be read) leaves it hidden too. Pure.
///
/// [`toggle_dock`]: super::visibility::toggle_dock
pub fn show_new_dock_window(label: &str, primary_visible: Option<bool>) -> bool {
    label == DOCK_LABEL || primary_visible == Some(true)
}

/// Whether the primary's dock window is visible; `None` when there is none,
/// or (after a warning) when its state cannot be read. Main thread.
fn primary_visibility(app: &AppHandle) -> Option<bool> {
    let primary = app.get_webview_window(DOCK_LABEL)?;
    match primary.is_visible() {
        Ok(visible) => Some(visible),
        Err(e) => {
            log::warn!("could not read whether the primary dock window is visible: {e}");
            None
        }
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
