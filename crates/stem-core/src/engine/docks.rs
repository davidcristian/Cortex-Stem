//! The per-dock facts: the dock windows the app layer registered, the
//! fullscreen fact and the overlap rule for each dock's panel.

use std::collections::BTreeMap;

use super::DockEngine;
use crate::model::{DockFacts, MonitorId, Rect};
use crate::overlap;

/// One dock window as the app layer registered it, keyed by its
/// label in [`DockEngine::panels`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct DockPanel {
    /// The monitor the window sits on; `None` while the app layer could not
    /// tell, in which case the dock has no entry in `DockState::docks`.
    pub(super) monitor: Option<MonitorId>,
    /// Where its panel rests, in the windows' coordinate space;
    /// `None` before the page's first report and while the window is hidden.
    pub(super) rect: Option<Rect>,
    /// `DockFacts::overlapped` for this panel, recomputed by
    /// [`DockEngine::apply`] when `overlap_dirty` was set by the input.
    overlapped: bool,
}

impl DockEngine {
    /// Tell the engine which process is ours: its windows never
    /// count as overlapping the dock. Set once at start by the service; the
    /// next input that touches the overlap rule applies it.
    pub fn set_own_process_id(&mut self, process_id: Option<u32>) {
        if self.own_process_id != process_id {
            self.own_process_id = process_id;
            self.overlap_dirty = true;
        }
    }

    /// The platform's latest fullscreen verdict.
    pub fn fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// The monitor the latest fullscreen verdict names; `None`
    /// while nothing is fullscreen or the platform could not tell.
    pub fn fullscreen_monitor(&self) -> Option<&MonitorId> {
        self.fullscreen_monitor.as_ref()
    }

    /// Where the panel of the dock window `label` rests, as last fed in;
    /// `None` while that window is hidden, before its page's first
    /// report, or for a label the app layer never registered.
    pub fn dock_panel(&self, label: &str) -> Option<Rect> {
        self.panels.get(label).and_then(|p| p.rect)
    }

    /// The monitor the dock window `label` sits on, as the app layer last
    /// said; `None` for an unknown label or an unplaced window.
    pub fn dock_monitor(&self, label: &str) -> Option<&MonitorId> {
        self.panels.get(label).and_then(|p| p.monitor.as_ref())
    }

    /// Whether a window of another process covers the panel of the dock
    /// window `label`, as of the last input; `false` for a label
    /// without a panel.
    pub fn overlapped(&self, label: &str) -> bool {
        self.panels.get(label).is_some_and(|p| p.overlapped)
    }

    /// [`overlap::overlapped`] over the tracked windows, for every panel as
    /// last reported; `false` for a dock without a rect.
    pub(super) fn compute_overlapped(&mut self) {
        let windows = &self.windows;
        let own_process_id = self.own_process_id;
        for panel in self.panels.values_mut() {
            panel.overlapped = panel.rect.as_ref().is_some_and(|rect| {
                overlap::overlapped(rect, windows.values().map(|t| &t.raw), own_process_id)
            });
        }
    }

    /// The monitor the app layer treats as primary (the flagged one, else
    /// the first), whose dock takes a fullscreen verdict that names no
    /// monitor.
    fn primary_monitor_id(&self) -> Option<&MonitorId> {
        self.monitors
            .iter()
            .find(|m| m.is_primary)
            .or_else(|| self.monitors.first())
            .map(|m| &m.id)
    }

    /// `DockState::docks`: one entry per monitor that has a
    /// registered dock window, sorted by monitor id. `fullscreen` is the
    /// verdict when it names this monitor, or names none and this is the
    /// primary; `overlapped` is the panel's (any panel's, should two share
    /// a monitor). A panel whose monitor is unknown yields no entry.
    pub(super) fn dock_facts(&self) -> Vec<DockFacts> {
        let primary = self.primary_monitor_id();
        let mut by_monitor: BTreeMap<&MonitorId, DockFacts> = BTreeMap::new();
        for panel in self.panels.values() {
            let Some(monitor) = &panel.monitor else {
                continue;
            };
            let fullscreen = self.fullscreen
                && match &self.fullscreen_monitor {
                    Some(target) => target == monitor,
                    None => primary == Some(monitor),
                };
            let facts = by_monitor.entry(monitor).or_insert_with(|| DockFacts {
                monitor: monitor.clone(),
                fullscreen,
                overlapped: false,
            });
            facts.overlapped |= panel.overlapped;
        }
        by_monitor.into_values().collect()
    }
}
