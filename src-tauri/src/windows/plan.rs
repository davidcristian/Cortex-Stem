//! Which dock windows should exist, with their labels and their pages.
//! Pure.

use std::fmt::Write as _;

use stem_core::settings::MonitorMode;
use stem_core::{MonitorId, MonitorInfo};

use super::DOCK_LABEL;

/// One dock window that should exist: its label and the monitor it sits on
/// (`None` only for the primary's window while no monitor is known).
#[derive(Debug, Clone, PartialEq)]
pub struct DockPlan {
    pub label: String,
    pub monitor: Option<MonitorInfo>,
}

/// The label of the dock window on monitor `id` other than the primary's:
/// `dock-` and the id with every character outside `[A-Za-z0-9_-]` dropped
/// (`\\.\DISPLAY3` → `dock-DISPLAY3`; Tauri labels allow `a-zA-Z0-9-/:_`).
/// Pure.
pub fn dock_label(id: &MonitorId) -> String {
    let mut label = String::with_capacity(DOCK_LABEL.len() + 1 + id.0.len());
    label.push_str(DOCK_LABEL);
    label.push('-');
    label.extend(
        id.0.chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-'),
    );
    label
}

/// `value` percent-encoded for a URL query (every byte outside the
/// unreserved set `A-Za-z0-9-_.~`), so `URLSearchParams.get('monitor')`
/// gives the page the monitor id back exactly (`\\.\DISPLAY3` →
/// `%5C%5C.%5CDISPLAY3`). Pure.
pub fn query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            // Writing to a String cannot fail.
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// The page a dock window loads: the primary's has no query string, the
/// others carry their monitor id (`docs/ipc/windows.md`). Pure.
pub fn dock_page(label: &str, monitor: Option<&MonitorId>) -> String {
    match monitor {
        Some(id) if label != DOCK_LABEL => format!("dock.html?monitor={}", query_value(&id.0)),
        _ => "dock.html".to_owned(),
    }
}

/// Which dock windows should exist for `mode` over `monitors`: the
/// primary's window (`dock`) on [`primary_monitor`] always, even with no
/// monitor known; with `All`, one more per other monitor, labelled by
/// [`dock_label`]. A monitor whose label collides with an earlier one's is
/// left out (the caller logs it). Pure.
pub fn plan_docks(mode: MonitorMode, monitors: &[MonitorInfo]) -> Vec<DockPlan> {
    let primary = primary_monitor(monitors);
    let mut plan = vec![DockPlan {
        label: DOCK_LABEL.to_owned(),
        monitor: primary.clone(),
    }];
    if mode == MonitorMode::Primary {
        return plan;
    }
    let primary_id = primary.map(|m| m.id);
    for monitor in monitors {
        if primary_id.as_ref() == Some(&monitor.id) {
            continue;
        }
        let label = dock_label(&monitor.id);
        if plan.iter().any(|p| p.label == label) {
            continue;
        }
        plan.push(DockPlan {
            label,
            monitor: Some(monitor.clone()),
        });
    }
    plan
}

fn primary_monitor(monitors: &[MonitorInfo]) -> Option<MonitorInfo> {
    monitors
        .iter()
        .find(|m| m.is_primary)
        .or_else(|| monitors.first())
        .cloned()
}

/// `monitor` for a log line.
pub(super) fn describe_monitor(monitor: Option<&MonitorInfo>) -> String {
    match monitor {
        Some(m) => format!(
            "{} ({}x{} at {},{}, scale {}{})",
            m.id.0,
            m.bounds.width,
            m.bounds.height,
            m.bounds.x,
            m.bounds.y,
            m.scale_factor,
            if m.is_primary { ", primary" } else { "" }
        ),
        None => "no known monitor (Tauri's view of the primary)".to_owned(),
    }
}
