//! The renderable snapshot, `DockState`: the items in their order, with
//! their titles and icons.

use std::collections::HashMap;

use super::windows::Tracked;
use super::{DockEngine, app_id_for};
use crate::model::{
    AppId, DockItem, DockItemKind, DockState, IconKey, PowerStatus, VolumeStatus, WindowInfo,
};

impl DockEngine {
    /// Build the renderable snapshot.
    pub fn snapshot(&self) -> DockState {
        let s = &self.settings;
        let mut items: Vec<DockItem> = Vec::new();
        let mut seps = 0u32;
        let mut separator = |items: &mut Vec<DockItem>| {
            if !s.items.separators {
                return;
            }
            seps += 1;
            items.push(fixed_item(
                DockItemKind::Separator,
                format!("sep:{seps}"),
                "",
                None,
            ));
        };

        // 1. Fixed left group.
        let left_start = items.len();
        if s.items.launcher {
            items.push(fixed_item(
                DockItemKind::Launcher,
                "fixed:launcher".into(),
                "Start",
                Some(IconKey::shell("start")),
            ));
        }
        if s.items.task_view {
            items.push(fixed_item(
                DockItemKind::TaskView,
                "fixed:task-view".into(),
                "Task View",
                Some(IconKey::shell("task-view")),
            ));
        }
        if s.items.show_desktop {
            items.push(fixed_item(
                DockItemKind::ShowDesktop,
                "fixed:show-desktop".into(),
                "Show Desktop",
                Some(IconKey::shell("show-desktop")),
            ));
        }
        let has_left = items.len() > left_start;

        // Group shown windows by app once.
        let mut by_app: HashMap<AppId, Vec<&Tracked>> = HashMap::new();
        for t in self.shown_windows() {
            by_app.entry(app_id_for(&t.raw)).or_default().push(t);
        }

        // 2. Pinned apps.
        let mut pinned_items = Vec::new();
        for pin in &s.pinned {
            let windows = by_app.remove(&pin.id).unwrap_or_default();
            pinned_items.push(self.app_item(&pin.id, true, pin.label.as_deref(), &windows));
        }

        // 3. Running, unpinned apps, but the hidden ones. The pins
        // above took their windows first, so a pinned id that is also
        // listed as hidden still has its item.
        let mut running_items = Vec::new();
        for id in &self.running_order {
            if let Some(windows) = by_app.remove(id)
                && !s.is_hidden(id)
            {
                running_items.push(self.app_item(id, false, None, &windows));
            }
        }

        if has_left && (!pinned_items.is_empty() || !running_items.is_empty()) {
            separator(&mut items);
        }
        let has_pinned = !pinned_items.is_empty();
        items.extend(pinned_items);
        if has_pinned && !running_items.is_empty() {
            separator(&mut items);
        }
        items.extend(running_items);

        // 4. Fixed trailing group: Quick Settings, Notification Center,
        // volume, battery, clock, and the Recycle Bin last (Windows' corner
        // order for the volume, battery and clock), with one separator
        // before the group and none inside it (like the left group).
        let mut trailing = Vec::new();
        if s.items.quick_settings {
            trailing.push(fixed_item(
                DockItemKind::QuickSettings,
                "fixed:quick-settings".into(),
                "Quick Settings",
                Some(IconKey::shell("quick-settings")),
            ));
        }
        if s.items.notification_center {
            trailing.push(fixed_item(
                DockItemKind::NotificationCenter,
                "fixed:notification-center".into(),
                "Notification Center",
                Some(IconKey::shell("notification-center")),
            ));
        }
        // Shown whenever the setting is on: without an output device its
        // face and title say so.
        if s.items.volume {
            trailing.push(fixed_item(
                DockItemKind::Volume,
                "fixed:volume".into(),
                &volume_title(self.volume.as_ref()),
                Some(IconKey::shell("volume")),
            ));
        }
        // A machine without a system battery (a desktop PC) never shows the
        // battery item, whatever the setting says.
        if s.items.battery && self.power.has_battery {
            trailing.push(fixed_item(
                DockItemKind::Battery,
                "fixed:battery".into(),
                &battery_title(&self.power),
                Some(IconKey::shell("battery")),
            ));
        }
        if s.items.clock {
            trailing.push(fixed_item(
                DockItemKind::Clock,
                "fixed:clock".into(),
                "Clock",
                Some(IconKey::shell("clock")),
            ));
        }
        if s.items.recycle_bin {
            let icon = if self.recycle_bin.is_empty() {
                "recycle-bin-empty"
            } else {
                "recycle-bin-full"
            };
            trailing.push(fixed_item(
                DockItemKind::RecycleBin,
                "fixed:recycle-bin".into(),
                "Recycle Bin",
                Some(IconKey::shell(icon)),
            ));
        }
        if !trailing.is_empty() && !items.is_empty() {
            separator(&mut items);
        }
        items.extend(trailing);

        DockState {
            revision: self.revision,
            items,
            foreground: self.foreground,
            docks: self.dock_facts(),
            power: self.power,
            volume: self.volume,
            clock_format: self.clock_format.clone(),
        }
    }

    fn app_item(
        &self,
        id: &AppId,
        pinned: bool,
        label: Option<&str>,
        windows: &[&Tracked],
    ) -> DockItem {
        let identity = self.identities.get(id);
        let first = windows.first().map(|t| &t.raw);
        let title = label
            .map(str::to_owned)
            .or_else(|| identity.map(|i| i.display_name.clone()))
            .or_else(|| first.map(|w| w.title.clone()))
            .unwrap_or_else(|| id.as_str().to_owned());
        let icon_key = identity
            .and_then(|i| i.icon_key.clone())
            .or_else(|| first.and_then(|w| w.icon_key.clone()));
        let infos: Vec<WindowInfo> = windows
            .iter()
            .map(|t| WindowInfo {
                id: t.raw.id,
                title: t.raw.title.clone(),
                is_minimized: t.raw.is_minimized,
                is_active: self.foreground == Some(t.raw.id),
                monitor: t.raw.monitor.clone(),
                icon_key: t.raw.icon_key.clone(),
                needs_attention: t.attention,
            })
            .collect();
        DockItem {
            key: format!("app:{}", id.as_str()),
            kind: DockItemKind::App,
            app_id: Some(id.clone()),
            title,
            icon_key,
            pinned,
            is_active: infos.iter().any(|w| w.is_active),
            needs_attention: infos.iter().any(|w| w.needs_attention),
            windows: infos,
            badge: None,
        }
    }
}

/// The battery item's title and accessible name (docs/ipc/widget-items.md
/// "Clock and battery items"): `Battery 73 %`, then `, charging` while
/// charging or `, plugged in` on AC power without charging, then `, battery
/// saver on` while battery saver is on; `Battery` alone, whatever else is
/// known, while the percent is not known.
pub(super) fn battery_title(power: &PowerStatus) -> String {
    let Some(percent) = power.percent else {
        return "Battery".to_owned();
    };
    let mut title = format!("Battery {percent} %");
    if power.charging {
        title.push_str(", charging");
    } else if power.on_ac {
        title.push_str(", plugged in");
    }
    if power.saver {
        title.push_str(", battery saver on");
    }
    title
}

/// The volume item's title and accessible name (docs/ipc/widget-items.md
/// "The volume item"): `Volume 45 %`, `Volume muted` while muted
/// (whatever the level), `No audio output` without a default output
/// device.
pub(super) fn volume_title(volume: Option<&VolumeStatus>) -> String {
    match volume {
        None => "No audio output".to_owned(),
        Some(v) if v.muted => "Volume muted".to_owned(),
        Some(v) => format!("Volume {} %", v.percent),
    }
}

fn fixed_item(kind: DockItemKind, key: String, title: &str, icon_key: Option<IconKey>) -> DockItem {
    DockItem {
        key,
        kind,
        app_id: None,
        title: title.to_owned(),
        icon_key,
        pinned: false,
        windows: Vec::new(),
        is_active: false,
        needs_attention: false,
        badge: None,
    }
}
