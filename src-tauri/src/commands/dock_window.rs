//! The commands only a dock window may call: the reports of its page and
//! the release of the keyboard focus.

use stem_core::Rect;
use stem_core::settings::DockPosition;
use tauri::{AppHandle, Manager, State};

use super::{CmdResult, checked};
use crate::AppService;
use crate::windows::{CssViewport, PanelReport};

/// The dock UI reports the size of its visible content (CSS pixels) so the
/// window can take its thickness from it: the height on a horizontal dock
/// (`bottom`, `top`), the width on a vertical one (`left`, `right`); the
/// window stays as long as its work area along the edge. See
/// `docs/ipc/sizing-protocol.md`. `edge` is the edge the page laid itself
/// out for (docs/ipc/report-provenance.md, v1.18): a report for another
/// edge than the window's is dropped there. Only a dock window may call
/// it, with finite numbers; the range is clamped in
/// `windows::apply_content_size`. Synchronous on purpose: it touches the
/// window and must run on the main thread.
#[tauri::command]
pub(super) fn set_dock_content_size(
    app: AppHandle,
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
    edge: DockPosition,
) -> CmdResult<()> {
    let args = (window.label().to_owned(), width, height, edge);
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "set_dock_content_size",
            &args,
            Err::<(), _>("only dock windows can be resized"),
        );
    }
    if !(width.is_finite() && height.is_finite()) {
        return checked(
            "set_dock_content_size",
            &args,
            Err::<(), _>("width and height must be finite numbers"),
        );
    }
    checked(
        "set_dock_content_size",
        &args,
        crate::windows::apply_content_size(&app, &window, width, height, edge),
    )
}

/// The dock UI reports where it accepts clicks (CSS px, window-local; see
/// `docs/ipc/hit-region.md`), laid out for `edge`
/// (docs/ipc/report-provenance.md: a report for another edge than the
/// window's is dropped). Only a dock window may call it; the rects must
/// have non-negative sizes and `envelope` must contain `panel`.
/// Synchronous: it reads the window's scale factor on the main thread and
/// the platform call only hands the region to its tracking thread.
#[tauri::command]
pub(super) fn set_dock_hit_region(
    app: AppHandle,
    window: tauri::WebviewWindow,
    panel: Rect,
    envelope: Rect,
    edge: DockPosition,
) -> CmdResult<()> {
    let args = (window.label().to_owned(), panel, envelope, edge);
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "set_dock_hit_region",
            &args,
            Err::<(), _>("only dock windows have a hit region"),
        );
    }
    if let Err(e) = crate::windows::validate_hit_region(panel, envelope) {
        return checked("set_dock_hit_region", &args, Err::<(), _>(e));
    }
    checked(
        "set_dock_hit_region",
        &args,
        crate::windows::apply_hit_region(&app, &window, panel, envelope, edge),
    )
}

/// The dock UI reports where its panel rests (CSS px relative to
/// `viewport`, whatever the auto-hide phase; see
/// `docs/ipc/dock-panel.md`), so the engine can tell when a window
/// overlaps it; the app layer hands it on with the calling window's label
/// and monitor, so each dock has its own fact. `edge` and `viewport` say
/// what the page measured it in (docs/ipc/report-provenance.md, v1.18): a
/// report for another edge than the window's is dropped, and the rect is
/// re-anchored to the window's viewport of the moment before it is
/// converted. Only a dock window may call it; the rect must have
/// non-negative sizes and the viewport a positive, finite width and
/// height. Synchronous: it reads the window's position, size and scale
/// factor on the main thread, and the service call only queues the
/// converted rect for its worker.
#[tauri::command]
pub(super) fn set_dock_panel_rect(
    app: AppHandle,
    window: tauri::WebviewWindow,
    rect: Rect,
    edge: DockPosition,
    viewport: CssViewport,
) -> CmdResult<()> {
    let args = (window.label().to_owned(), rect, edge, viewport);
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "set_dock_panel_rect",
            &args,
            Err::<(), _>("only dock windows have a panel"),
        );
    }
    if let Err(e) = crate::windows::validate_panel_rect(rect).and_then(|()| viewport.validate()) {
        return checked("set_dock_panel_rect", &args, Err::<(), _>(e));
    }
    let report = PanelReport {
        rect,
        edge,
        viewport,
    };
    checked(
        "set_dock_panel_rect",
        &args,
        crate::windows::apply_dock_panel(&app, &window, report),
    )
}

/// The dock UI reports that auto-hide slid its panel out, or brought it
/// back (`docs/ipc/toggle-while-hidden.md`), so the tray click and
/// the toggle shortcut can bring the panel back instead of hiding the
/// window. Only a dock window may call it. Synchronous: a flag in managed
/// state, nothing else.
#[tauri::command]
pub(super) fn set_dock_auto_hidden(
    app: AppHandle,
    window: tauri::WebviewWindow,
    hidden: bool,
) -> CmdResult<()> {
    let args = (window.label().to_owned(), hidden);
    if !window.label().starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "set_dock_auto_hidden",
            &args,
            Err::<(), _>("only dock windows auto-hide"),
        );
    }
    checked(
        "set_dock_auto_hidden",
        &args,
        crate::windows::set_auto_hidden(&app, &window, hidden),
    )
}

/// The calling dock window gives the keyboard focus back (Escape, or a
/// second press of the focus shortcut through `dock://release-focus`;
/// `docs/ipc/keyboard-entry.md`): the window that was in front
/// when the focus shortcut was pressed (an app's, or the settings window)
/// comes back to the foreground, if the focus shortcut's session is this
/// dock's and that window still exists (a click on a dock forgets it);
/// otherwise the foreground is left alone and the command resolves. A call
/// from a dock that does not hold the session leaves it alone while the
/// holder is still in front. Only a dock window may call it. `async`: the
/// activation is the foreground chain, which must not run on the main thread.
#[tauri::command]
pub(super) async fn release_dock_focus(
    window: tauri::WebviewWindow,
    service: State<'_, AppService>,
    focus: State<'_, crate::windows::DockFocus>,
) -> CmdResult<()> {
    let label = window.label().to_owned();
    if !label.starts_with(crate::windows::DOCK_LABEL) {
        return checked(
            "release_dock_focus",
            &label,
            Err::<(), _>("only dock windows take the keyboard focus"),
        );
    }
    checked(
        "release_dock_focus",
        &label,
        crate::windows::release_dock_focus(
            window.app_handle(),
            &focus,
            service.0.platform().as_ref(),
            &label,
        ),
    )
}
