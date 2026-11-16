//! The state kept for each dock window ([`DockWindowState`], in
//! [`DockGeometry`]), and the log lines for a report that is dropped.

use std::collections::{HashMap, HashSet};

use parking_lot::Mutex;
use stem_core::settings::{DockPosition, MonitorMode};
use stem_core::{MonitorId, Rect};
use stem_platform::rules::appbar::reserved_thickness;

use super::DOCK_LABEL;
use super::placement::DockContent;
use super::viewport::PanelReport;

/// What one dock window's UI reported last (CSS px) and where this module
/// put the window, kept per label in [`DockGeometry`].
#[derive(Debug, Default, Clone)]
pub struct DockWindowState {
    /// The monitor the window was placed on last; `None` while the platform
    /// has reported no monitors yet (the window then sits where Tauri's own
    /// view of the primary monitor put it, and the engine gets no monitor
    /// for it).
    pub(super) monitor: Option<MonitorId>,
    /// The latest `set_dock_content_size`, clamped, with the edge the page
    /// measured it for (the edge the window was on then, or the
    /// report was dropped); `None` before the UI's first report.
    pub(super) content: Option<DockContent>,
    /// The edge the window was placed on last, taken as soon as the window
    /// has its size for that edge ([`place`]); `None` before its first
    /// placement. Says which side of the window is its thickness when no
    /// content was reported yet ([`thickness_of`]).
    ///
    /// [`place`]: super::layout::place
    /// [`thickness_of`]: super::placement::thickness_of
    pub(super) edge: Option<DockPosition>,
    /// `(panel, envelope)` of the latest `set_dock_hit_region`; applied
    /// while `behavior.clickThrough` is on and replayed when it is turned
    /// back on.
    pub(super) hit_region_css: Option<(Rect, Rect)>,
    /// The resting panel of the latest `set_dock_panel_rect` with
    /// the edge and the viewport the page measured it in; converted
    /// again, re-anchored to the window's viewport of the moment
    /// ([`PanelReport::now`]), whenever the window moves or is shown.
    pub(super) panel: Option<PanelReport>,
    /// From that panel's far side to the viewport's edge side, CSS px, read
    /// against the viewport the page measured it in: the
    /// thickness of the band *Reserve screen space* asks for, scaled when
    /// the request is made (`appbar::request_for`). The distance to the edge
    /// side is what re-anchoring keeps, so it holds in the window's viewport
    /// of any moment: a report handled after a resize (a drop, a smaller
    /// icon size) gives the band its right thickness at once, and a window
    /// that grows for a drag never asks for a thicker band. Kept as it was
    /// only when the rect does not lie inside its own viewport across the
    /// edge (`reserved_thickness` is `None`), which a page laid out as
    /// `docs/ipc/sizing-protocol.md` says never sends. Dropped with the
    /// panel rect.
    pub(super) reserve_css: Option<i32>,
    /// The window's scale factor as last seen (at creation, then on every
    /// `ScaleFactorChanged`), so the log line of a change can say what it
    /// changed from.
    pub(super) scale: Option<f64>,
}

impl DockWindowState {
    /// The edge the window was placed on last when a report the page
    /// measured for `edge` does not belong to it: the page laid
    /// itself out for another edge, before it saw a position change, and
    /// the report is dropped. `None` when the report is current, and for any
    /// edge before the window's first placement (a window is placed when it
    /// is created, before its page loads).
    pub(super) fn stale_for(&self, edge: DockPosition) -> Option<DockPosition> {
        self.edge.filter(|placed| *placed != edge)
    }

    /// Take a `set_dock_content_size` measured for `content.edge`, unless it
    /// is stale ([`Self::stale_for`]; `Err` with the window's edge) and
    /// `in_effect`, the edge the windows are laid out for ([`dock_edge`]),
    /// is not its edge either. A report for the edge in effect is never
    /// stale: the page measured it after it saw the position change. The
    /// window is not on that edge yet only when its move there failed
    /// ([`on_position_setting`] logs it; a getter or a resize failed) or has
    /// not run yet: `Ok(Some(placed))`, and the [`layout`] that follows the
    /// report is the move, retried. Without that, every report for the edge
    /// in effect would be dropped until an unrelated placement (a monitor or
    /// DPI change) moved the window. `Ok(None)` for a report on the window's
    /// edge.
    ///
    /// [`dock_edge`]: super::relayout::dock_edge
    /// [`on_position_setting`]: super::changes::on_position_setting
    /// [`layout`]: super::layout::layout
    pub(super) fn take_content(
        &mut self,
        content: DockContent,
        in_effect: DockPosition,
    ) -> Result<Option<DockPosition>, DockPosition> {
        let stale = self.stale_for(content.edge);
        if let Some(placed) = stale {
            if content.edge != in_effect {
                return Err(placed);
            }
        }
        self.content = Some(content);
        Ok(stale)
    }

    /// Take a `set_dock_hit_region` measured for `edge`, unless it is stale
    /// ([`Self::stale_for`]; `Err` with the window's edge).
    pub(super) fn take_hit_region(
        &mut self,
        panel: Rect,
        envelope: Rect,
        edge: DockPosition,
    ) -> Result<(), DockPosition> {
        if let Some(placed) = self.stale_for(edge) {
            return Err(placed);
        }
        self.hit_region_css = Some((panel, envelope));
        Ok(())
    }

    /// Take a `set_dock_panel_rect`, unless it is stale
    /// ([`Self::stale_for`]; `Err` with the window's edge), with the band's
    /// thickness read against the report's own viewport: `Ok(Some(css))`,
    /// or `Ok(None)` when the rect does not lie inside that viewport across
    /// the edge, which keeps the thickness as it was.
    pub(super) fn take_panel(&mut self, report: PanelReport) -> Result<Option<i32>, DockPosition> {
        if let Some(placed) = self.stale_for(report.edge) {
            return Err(placed);
        }
        self.panel = Some(report);
        let thickness = reserved_thickness(report.viewport.whole(), report.rect, report.edge);
        if thickness.is_some() {
            self.reserve_css = thickness;
        }
        Ok(thickness)
    }
}

/// The dock windows that exist, by label, with what each one's UI reported,
/// plus the settings this module applies, as currently in effect.
/// Kept in Tauri's managed state so a monitor change can redo the layouts
/// and a settings change can apply the hit regions after the fact.
#[derive(Debug, Default)]
pub struct DockGeometry {
    pub(super) docks: Mutex<HashMap<String, DockWindowState>>,
    /// `behavior.clickThrough` as currently in effect, so a settings event
    /// only acts on a change.
    pub(super) click_through: Mutex<Option<bool>>,
    /// `settings.monitors` as the windows were last reconciled for, so a
    /// settings event only rebuilds the set on a change.
    pub(super) mode: Mutex<Option<MonitorMode>>,
    /// `settings.position` as the windows are laid out for, so a
    /// settings event only moves them on a change; [`dock_edge`] reads it
    /// for every placement.
    ///
    /// [`dock_edge`]: super::relayout::dock_edge
    pub(super) position: Mutex<Option<DockPosition>>,
    /// The labels of the dock windows [`close_dock_window`] destroyed whose
    /// `WindowEvent::Destroyed` has not arrived yet. Tauri only posts the
    /// destroy, and keeps the label (and `get_webview_window`) until then,
    /// so such a window is neither live (it has no state in `docks`) nor
    /// gone (its label cannot be built again) until that event.
    ///
    /// [`close_dock_window`]: super::reconcile::close_dock_window
    pub(super) closing: Mutex<HashSet<String>>,
    /// The closing labels a reconcile wanted again while they were closing
    /// ([`reconcile_docks`]): their `Destroyed` runs the reconcile again,
    /// which creates them anew.
    ///
    /// [`reconcile_docks`]: super::reconcile::reconcile_docks
    pub(super) wanted_again: Mutex<HashSet<String>>,
}

impl DockGeometry {
    /// A copy of the state of the dock window `label`, if it exists.
    pub(super) fn dock(&self, label: &str) -> Option<DockWindowState> {
        self.docks.lock().get(label).cloned()
    }

    /// Change the state of the dock window `label`, if it exists.
    pub(super) fn update<R>(
        &self,
        label: &str,
        f: impl FnOnce(&mut DockWindowState) -> R,
    ) -> Option<R> {
        self.docks.lock().get_mut(label).map(f)
    }

    /// The labels of the dock windows that exist, the primary's first, the
    /// rest sorted.
    pub(super) fn labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self.docks.lock().keys().cloned().collect();
        labels.sort_by(|a, b| (a != DOCK_LABEL).cmp(&(b != DOCK_LABEL)).then(a.cmp(b)));
        labels
    }

    /// The dock window `label` was just destroyed ([`close_dock_window`]);
    /// its `Destroyed` is still to come ([`Self::closing`]).
    ///
    /// [`close_dock_window`]: super::reconcile::close_dock_window
    pub(super) fn start_closing(&self, label: &str) {
        self.closing.lock().insert(label.to_owned());
    }

    /// Whether the dock window `label` is still closing; if it is, a
    /// reconcile that wants it is remembered ([`Self::wanted_again`]) and
    /// runs again once the window is gone.
    pub(super) fn defer_if_closing(&self, label: &str) -> bool {
        if !self.closing.lock().contains(label) {
            return false;
        }
        self.wanted_again.lock().insert(label.to_owned());
        true
    }

    /// The dock window `label` is gone (`WindowEvent::Destroyed`): `None`
    /// when it was not closing (the quit destroys every window), else
    /// whether a reconcile wanted it again meanwhile.
    pub(super) fn finish_closing(&self, label: &str) -> Option<bool> {
        if !self.closing.lock().remove(label) {
            return None;
        }
        Some(self.wanted_again.lock().remove(label))
    }
}

/// One `debug` line for a report the page measured for `edge` while the
/// window is on `placed`: dropped, the page reports for the new
/// edge next.
pub(super) fn log_stale(label: &str, report: &str, edge: DockPosition, placed: DockPosition) {
    log::debug!(
        "dock window {label:?}: {report} measured for the {edge:?} edge dropped; the window is on the {placed:?} edge"
    );
}

/// One `debug` line for a report from a dock window that has no state: it
/// is closing ([`close_dock_window`] forgot it; Tauri destroys it at the
/// next turn of the event loop), so the report is dropped. A window gets
/// its state right after it is built, before its page loads.
///
/// [`close_dock_window`]: super::reconcile::close_dock_window
pub(super) fn log_closing(label: &str, report: &str) {
    log::debug!("dock window {label:?}: {report} dropped; the window is closing");
}
