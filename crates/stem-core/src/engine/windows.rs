//! The windows the engine tracks: how it groups them by app, which ones
//! the dock shows, and which ones each dock window lists.

use super::DockEngine;
use crate::model::{AppId, DockItem, RawWindow, WindowId};
use crate::settings::MonitorMode;

/// Derive the grouping identity of a window from what the platform reported.
pub fn app_id_for(raw: &RawWindow) -> AppId {
    if let Some(aumid) = raw
        .app_user_model_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return AppId::for_app_user_model_id(aumid);
    }
    if let Some(path) = &raw.exe_path {
        return AppId::for_exe(path);
    }
    AppId::for_window_class(&raw.class_name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Tracked {
    pub(super) raw: RawWindow,
    /// First-seen sequence number; drives stable ordering.
    pub(super) seq: u64,
    pub(super) attention: bool,
}

impl DockEngine {
    pub fn window(&self, id: WindowId) -> Option<&RawWindow> {
        self.windows.get(&id).map(|t| &t.raw)
    }

    /// Shown windows of `app_id` in first-seen order, i.e. the order the
    /// app's item lists them. Hidden (cloaked, invisible) windows are not
    /// included.
    pub fn windows_of(&self, app_id: &AppId) -> Vec<WindowId> {
        self.shown_windows()
            .into_iter()
            .filter(|t| app_id_for(&t.raw) == *app_id)
            .map(|t| t.raw.id)
            .collect()
    }

    /// Shown windows of `app_id` that the dock window `dock` lists, in
    /// first-seen order: what a click on its item and its context menu act
    /// on. With `Settings.monitors = all` and
    /// `behavior.showOnlyCurrentMonitor` on, each dock's page lists only the
    /// windows on its own monitor (`itemsForMonitor` in
    /// src/lib/dock/monitor.ts), so only the windows on
    /// [`Self::dock_monitor`] are kept (a window whose monitor is unknown is
    /// on none). With one dock (`monitors = primary`), the setting off, or a
    /// dock whose monitor is unknown (an unplaced window, a label the app
    /// layer never registered): every shown window, as [`Self::windows_of`].
    pub fn windows_on_dock(&self, app_id: &AppId, dock: &str) -> Vec<WindowId> {
        let settings = &self.settings;
        let monitor = if settings.monitors == MonitorMode::All
            && settings.behavior.show_only_current_monitor
        {
            self.dock_monitor(dock)
        } else {
            None
        };
        self.shown_windows()
            .into_iter()
            .filter(|t| app_id_for(&t.raw) == *app_id)
            .filter(|t| monitor.is_none_or(|m| t.raw.monitor.as_ref() == Some(m)))
            .map(|t| t.raw.id)
            .collect()
    }

    /// The snapshot's item `item_key` as the dock window `dock` lists it:
    /// an app item keeps only the windows of
    /// [`Self::windows_on_dock`], and its `is_active` and `needs_attention`
    /// follow the windows left, as on that dock's page; any other item is
    /// the snapshot's. `None` for a key the snapshot does not have. The
    /// dock's context menu is built from it.
    pub fn dock_item(&self, item_key: &str, dock: &str) -> Option<DockItem> {
        let mut item = self
            .snapshot()
            .items
            .into_iter()
            .find(|i| i.key == item_key)?;
        if let Some(app_id) = &item.app_id {
            let listed = self.windows_on_dock(app_id, dock);
            item.windows.retain(|w| listed.contains(&w.id));
            item.is_active = item.windows.iter().any(|w| w.is_active);
            item.needs_attention = item.windows.iter().any(|w| w.needs_attention);
        }
        Some(item)
    }

    /// The name the item of `app_id` shows, or would show were it not
    /// hidden: its resolved display name, else the title of its
    /// first shown window (the one the item's title falls back to, see
    /// `app_item`). An app with tracked windows but none shown (all
    /// cloaked, say) has no item; its first-seen window's title is used
    /// then. `None` when the engine knows nothing of the app: no identity
    /// and no window. A pin's label is not consulted; the caller decides
    /// what a pinned app is called.
    pub fn app_name(&self, app_id: &AppId) -> Option<String> {
        if let Some(identity) = self.identities.get(app_id) {
            return Some(identity.display_name.clone());
        }
        self.app_window(app_id).map(|w| w.title.clone())
    }

    /// The window that stands for `app_id` when its item is not at hand:
    /// its first shown window (the one its item's title
    /// and icon fall back to, see `app_item`), else its first-seen tracked
    /// window (an app with none shown has no item). `None` when the engine
    /// tracks no window of the app.
    pub fn app_window(&self, app_id: &AppId) -> Option<&RawWindow> {
        let of_app = |t: &&Tracked| app_id_for(&t.raw) == *app_id;
        self.shown_windows()
            .into_iter()
            .find(of_app)
            .or_else(|| self.windows.values().filter(of_app).min_by_key(|t| t.seq))
            .map(|t| &t.raw)
    }

    /// Whether the dock lists `raw`: visible, and, while
    /// `behavior.showOnlyCurrentDesktop` is on, on the current virtual
    /// desktop. The platform's `on_current_desktop` decides that when it is
    /// known, whatever the cloak state: a suspended Store app or a
    /// window in a desktop-switch animation is cloaked on this desktop and
    /// stays listed. Without it a cloaked window counts as away.
    fn is_shown(&self, raw: &RawWindow) -> bool {
        if !raw.is_visible {
            return false;
        }
        if !self.settings.behavior.show_only_current_desktop {
            return true;
        }
        raw.on_current_desktop.unwrap_or(!raw.is_cloaked)
    }

    /// Windows that should appear in the dock, in first-seen order.
    pub(super) fn shown_windows(&self) -> Vec<&Tracked> {
        let mut v: Vec<&Tracked> = self
            .windows
            .values()
            .filter(|t| self.is_shown(&t.raw))
            .collect();
        v.sort_by_key(|t| t.seq);
        v
    }

    pub(super) fn rebuild_running_order(&mut self) {
        let mut present: Vec<AppId> = Vec::new();
        for t in self.shown_windows() {
            let id = app_id_for(&t.raw);
            if !present.contains(&id) {
                present.push(id);
            }
        }
        // Keep existing relative order for apps that are still running, then
        // append newcomers in first-seen order.
        let mut order: Vec<AppId> = self
            .running_order
            .iter()
            .filter(|id| present.contains(id))
            .cloned()
            .collect();
        for id in present {
            if !order.contains(&id) {
                order.push(id);
            }
        }
        self.running_order = order;
    }
}
