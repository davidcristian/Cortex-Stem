//! The dock engine: a pure, synchronous state machine.
//!
//! Feed it [`EngineInput`]s, ask it for a [`DockState`] snapshot, and ask it
//! what a click on an item should do. It never touches the OS, which is what
//! makes the project's behaviour testable on any host.
//!
//! Ordering rules for the snapshot (left to right):
//! 1. fixed left items (launcher, task view, show desktop), then a separator;
//! 2. pinned apps in the user's order, each showing its running windows;
//! 3. a separator, then running apps that are not pinned, in first-seen order;
//! 4. a separator, then the Recycle Bin.
//!
//! Separators are only emitted between non-empty groups.
//!
//! Invariants worth knowing when extending the engine:
//! - a window is grouped under the id [`app_id_for`] derives from its *current*
//!   facts, so a window whose AppUserModelID shows up late simply moves to the
//!   other item (first-seen order is kept per window, not per app);
//! - [`EngineOutcome::changed`] is defined by comparing snapshots (items,
//!   foreground and the per-dock facts in
//!   [`DockState::docks`]), not by what kind of input arrived, so inputs that
//!   only touch facts outside the snapshot (theme, launch targets, identities
//!   of apps that are not shown) never bump the revision. The monitor list
//!   is such a fact too, except that a `MonitorsChanged` bumps the revision
//!   when it moves a fullscreen verdict that names no monitor to another
//!   dock (the primary changed, or the list emptied, or it was reordered
//!   with no monitor flagged primary);
//! - `foreground` is the OS truth and is only ever an id the engine tracks. It
//!   may point at a window no item shows (cloaked, invisible); nothing is
//!   marked active then, and the item lights up as soon as the window is shown
//!   again without needing another `ForegroundChanged`.
//! - the per-dock facts (`DockState::docks`) come from the panels the
//!   app layer registers with [`EngineInput::DockPanelChanged`], one per dock
//!   window label; a panel whose monitor is unknown yields no entry. The
//!   snapshot lists one entry per monitor, sorted by monitor id;
//! - `fullscreen` is the platform's verdict, stored as is (with the
//!   monitor it names): the engine never derives it from window rects. It
//!   sets the fact on the dock of the verdict's monitor, or on the primary
//!   monitor's dock when the verdict names none;
//! - `overlapped` is the one thing the engine derives from window
//!   rects: whether a window of another process covers a dock's resting
//!   panel ([`crate::overlap`]), per panel. It is recomputed only after an
//!   input that can change it (a window's frame, visibility, minimised or
//!   cloaked state, a window coming or going, a resync, a panel itself),
//!   in O(windows × panels), and it is part of the snapshot, so a
//!   `WindowUpdated` that only moves a window bumps the revision exactly
//!   when a flag flips.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::events::{EngineInput, PlatformEvent};
use crate::model::{
    AppId, AppIdentity, DockFacts, DockItem, DockItemKind, DockState, IconKey, MonitorId,
    MonitorInfo, RawWindow, Rect, RecycleBinState, ShellAction, ThemeInfo, WindowId, WindowInfo,
};
use crate::overlap;
use crate::settings::{MonitorMode, Settings};

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

/// What the engine wants the app layer to do after applying an input.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineOutcome {
    /// The renderable snapshot (items, foreground or the per-dock facts in
    /// [`DockState::docks`]) differs from the one before this input;
    /// emit a new `dock://state`. Inputs that only change
    /// [`DockEngine::theme`], settings that do not alter items, or
    /// identities of apps that are not shown leave this `false` and do not
    /// advance the revision. So does a change of [`DockEngine::monitors`],
    /// unless it moves a fullscreen verdict that names no monitor to another
    /// dock (the primary changed, or the list emptied, or it was reordered
    /// with no monitor flagged primary).
    pub changed: bool,
    /// Apps the engine saw for the first time and has no [`AppIdentity`] for.
    /// Resolve them via the platform and feed back `EngineInput::IdentityResolved`.
    /// Each id is requested once; use [`DockEngine::unresolved_apps`] to retry.
    pub unresolved_apps: Vec<AppId>,
}

/// The effect of a primary click on a dock item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemAction {
    Launch(AppId),
    ActivateWindow(WindowId),
    MinimizeWindow(WindowId),
    Shell(ShellAction),
    /// Nothing sensible to do (unknown item, app without launch target, ...).
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tracked {
    raw: RawWindow,
    /// First-seen sequence number; drives stable ordering.
    seq: u64,
    attention: bool,
}

/// One dock window as the app layer registered it, keyed by its
/// label in [`DockEngine::panels`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct DockPanel {
    /// The monitor the window sits on; `None` while the app layer could not
    /// tell, in which case the dock has no entry in `DockState::docks`.
    monitor: Option<MonitorId>,
    /// Where its panel rests, in the windows' coordinate space;
    /// `None` before the page's first report and while the window is hidden.
    rect: Option<Rect>,
    /// `DockFacts::overlapped` for this panel, recomputed by
    /// [`DockEngine::apply`] when `overlap_dirty` was set by the input.
    overlapped: bool,
}

#[derive(Debug)]
pub struct DockEngine {
    settings: Settings,
    windows: BTreeMap<WindowId, Tracked>,
    identities: HashMap<AppId, AppIdentity>,
    identity_requested: HashSet<AppId>,
    /// First-seen order of apps that currently have windows.
    running_order: Vec<AppId>,
    foreground: Option<WindowId>,
    /// The platform's fullscreen verdict; see `DockFacts::fullscreen`.
    fullscreen: bool,
    /// The monitor the fullscreen verdict names; `None` when the
    /// platform could not tell, which points the fact at the primary
    /// monitor's dock.
    fullscreen_monitor: Option<MonitorId>,
    /// The dock windows the app layer registered, by label.
    panels: BTreeMap<String, DockPanel>,
    /// Our own process id, whose windows never count as overlapping. `None`
    /// when unknown (every window counts then).
    own_process_id: Option<u32>,
    /// Some panel's `overlapped` may be stale: recompute them all in
    /// [`Self::apply`].
    overlap_dirty: bool,
    monitors: Vec<MonitorInfo>,
    theme: ThemeInfo,
    recycle_bin: RecycleBinState,
    next_seq: u64,
    revision: u64,
}

impl DockEngine {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            windows: BTreeMap::new(),
            identities: HashMap::new(),
            identity_requested: HashSet::new(),
            running_order: Vec::new(),
            foreground: None,
            fullscreen: false,
            fullscreen_monitor: None,
            panels: BTreeMap::new(),
            own_process_id: None,
            overlap_dirty: false,
            monitors: Vec::new(),
            theme: ThemeInfo::default(),
            recycle_bin: RecycleBinState::default(),
            next_seq: 1,
            revision: 0,
        }
    }

    /// Tell the engine which process is ours: its windows never
    /// count as overlapping the dock. Set once at start by the service; the
    /// next input that touches the overlap rule applies it.
    pub fn set_own_process_id(&mut self, process_id: Option<u32>) {
        if self.own_process_id != process_id {
            self.own_process_id = process_id;
            self.overlap_dirty = true;
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn theme(&self) -> &ThemeInfo {
        &self.theme
    }

    pub fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    pub fn recycle_bin(&self) -> RecycleBinState {
        self.recycle_bin
    }

    /// The OS foreground window, if the engine tracks it. It may be a window
    /// that no item shows (cloaked or invisible).
    pub fn foreground(&self) -> Option<WindowId> {
        self.foreground
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

    pub fn window(&self, id: WindowId) -> Option<&RawWindow> {
        self.windows.get(&id).map(|t| &t.raw)
    }

    pub fn identity(&self, id: &AppId) -> Option<&AppIdentity> {
        self.identities.get(id)
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

    /// Apps (pinned or running) with no identity yet, regardless of whether
    /// they were requested before. Use for retries after a failed resolution.
    pub fn unresolved_apps(&self) -> Vec<AppId> {
        let mut ids: Vec<AppId> = self.settings.pinned.iter().map(|p| p.id.clone()).collect();
        ids.extend(self.running_order.iter().cloned());
        ids.sort();
        ids.dedup();
        ids.retain(|id| !self.identities.contains_key(id));
        ids
    }

    /// Apply one input. Cheap; call for every event.
    ///
    /// `changed` in the outcome is decided by comparing the snapshot before
    /// and after, see [`EngineOutcome::changed`].
    pub fn apply(&mut self, input: EngineInput) -> EngineOutcome {
        let before = self.snapshot();
        match input {
            EngineInput::Platform(ev) => self.apply_platform(ev),
            EngineInput::SettingsChanged(settings) => self.settings = settings,
            EngineInput::IdentityResolved { app_id, identity } => {
                self.identity_requested.remove(&app_id);
                self.identities.insert(app_id, identity);
            }
            EngineInput::IdentityUnavailable { app_id } => {
                // Only forget the request. If the app is still (or again)
                // present, `take_new_unresolved` below asks for it afresh.
                self.identity_requested.remove(&app_id);
            }
            EngineInput::DockPanelChanged {
                label,
                monitor,
                rect,
            } => {
                if monitor.is_none() && rect.is_none() {
                    // The window is gone (or was never placed and is
                    // hidden): nothing left to remember about the label.
                    self.panels.remove(&label);
                } else {
                    let panel = self.panels.entry(label).or_default();
                    if panel.rect != rect {
                        panel.rect = rect;
                        self.overlap_dirty = true;
                    }
                    panel.monitor = monitor;
                }
            }
        }
        self.rebuild_running_order();
        if self.overlap_dirty {
            self.overlap_dirty = false;
            self.compute_overlapped();
        }
        let unresolved_apps = self.take_new_unresolved();
        let changed = self.snapshot() != before;
        if changed {
            self.revision += 1;
        }
        EngineOutcome {
            changed,
            unresolved_apps,
        }
    }

    fn apply_platform(&mut self, ev: PlatformEvent) {
        match ev {
            PlatformEvent::WindowsSnapshot(list) => {
                let mut next = BTreeMap::new();
                for raw in list {
                    let id = raw.id;
                    let (seq, attention) = match self.windows.get(&id) {
                        Some(t) => (t.seq, t.attention),
                        None => (self.alloc_seq(), false),
                    };
                    next.insert(
                        id,
                        Tracked {
                            raw,
                            seq,
                            attention,
                        },
                    );
                }
                self.windows = next;
                // A resync can reveal that the foreground window is gone.
                self.foreground = self.foreground.filter(|id| self.windows.contains_key(id));
                self.overlap_dirty = true;
            }
            PlatformEvent::WindowAdded(raw) | PlatformEvent::WindowUpdated(raw) => self.upsert(raw),
            PlatformEvent::WindowRemoved(id) => {
                if self.windows.remove(&id).is_some() {
                    self.overlap_dirty = true;
                }
                if self.foreground == Some(id) {
                    self.foreground = None;
                }
            }
            PlatformEvent::ForegroundChanged(id) => {
                self.foreground = id.filter(|id| self.windows.contains_key(id));
                if let Some(t) = self.foreground.and_then(|id| self.windows.get_mut(&id)) {
                    // Activating a window clears its attention request.
                    t.attention = false;
                }
            }
            PlatformEvent::WindowAttention { id, flashing } => {
                // The foreground window already has the user's attention, and
                // only a foreground change clears the flag, so a flash of the
                // window in front would keep its icon wiggling. It is never
                // flagged; together with `ForegroundChanged` above, the
                // foreground window never needs attention.
                let in_front = self.foreground == Some(id);
                if let Some(t) = self.windows.get_mut(&id) {
                    t.attention = flashing && !in_front;
                }
            }
            PlatformEvent::MonitorsChanged(monitors) => self.monitors = monitors,
            PlatformEvent::ThemeChanged(theme) => self.theme = theme,
            PlatformEvent::RecycleBinChanged(state) => self.recycle_bin = state,
            // Part of the snapshot (through the docks' facts), so `apply`
            // bumps the revision only when some dock's fact changes.
            PlatformEvent::FullscreenChanged { active, monitor } => {
                self.fullscreen = active;
                self.fullscreen_monitor = if active { monitor } else { None };
            }
        }
    }

    fn upsert(&mut self, raw: RawWindow) {
        let id = raw.id;
        match self.windows.get_mut(&id) {
            Some(t) => {
                if overlap_facts(&t.raw) != overlap_facts(&raw) {
                    self.overlap_dirty = true;
                }
                t.raw = raw;
            }
            None => {
                let seq = self.alloc_seq();
                self.windows.insert(
                    id,
                    Tracked {
                        raw,
                        seq,
                        attention: false,
                    },
                );
                self.overlap_dirty = true;
            }
        }
    }

    /// [`overlap::overlapped`] over the tracked windows, for every panel as
    /// last reported; `false` for a dock without a rect.
    fn compute_overlapped(&mut self) {
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
    fn dock_facts(&self) -> Vec<DockFacts> {
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

    fn alloc_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
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
    fn shown_windows(&self) -> Vec<&Tracked> {
        let mut v: Vec<&Tracked> = self
            .windows
            .values()
            .filter(|t| self.is_shown(&t.raw))
            .collect();
        v.sort_by_key(|t| t.seq);
        v
    }

    fn rebuild_running_order(&mut self) {
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

    fn take_new_unresolved(&mut self) -> Vec<AppId> {
        let fresh: Vec<AppId> = self
            .unresolved_apps()
            .into_iter()
            .filter(|id| !self.identity_requested.contains(id))
            .collect();
        self.identity_requested.extend(fresh.iter().cloned());
        fresh
    }

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

        // 3. Running, unpinned apps.
        let mut running_items = Vec::new();
        for id in &self.running_order {
            if let Some(windows) = by_app.remove(id) {
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

        // 4. Recycle Bin.
        if s.items.recycle_bin {
            if !items.is_empty() {
                separator(&mut items);
            }
            let icon = if self.recycle_bin.is_empty() {
                "recycle-bin-empty"
            } else {
                "recycle-bin-full"
            };
            items.push(fixed_item(
                DockItemKind::RecycleBin,
                "fixed:recycle-bin".into(),
                "Recycle Bin",
                Some(IconKey::shell(icon)),
            ));
        }

        DockState {
            revision: self.revision,
            items,
            foreground: self.foreground,
            docks: self.dock_facts(),
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

    /// Decide what a primary click on `item_key` in the dock window `dock`
    /// should do.
    ///
    /// - fixed items map to their shell action;
    /// - an app with none of the windows that dock lists
    ///   ([`Self::windows_on_dock`]) is launched (if it can be): a pinned app
    ///   whose windows are all on other monitors starts a new one, as that
    ///   dock shows no window for it;
    /// - an app whose window is active is minimised (if configured), otherwise
    ///   activated;
    /// - an app with several windows cycles: the window after the active one,
    ///   or the first one. Only the windows that dock lists take part.
    pub fn primary_action(&self, item_key: &str, dock: &str) -> ItemAction {
        match item_key {
            "fixed:launcher" => return ItemAction::Shell(ShellAction::OpenStartMenu),
            "fixed:task-view" => return ItemAction::Shell(ShellAction::OpenTaskView),
            "fixed:show-desktop" => return ItemAction::Shell(ShellAction::ToggleShowDesktop),
            "fixed:recycle-bin" => return ItemAction::Shell(ShellAction::OpenRecycleBin),
            _ => {}
        }
        let Some(app_id) = item_key.strip_prefix("app:").map(|s| AppId(s.to_owned())) else {
            return ItemAction::None;
        };
        let windows = self.windows_on_dock(&app_id, dock);
        if windows.is_empty() {
            let launchable = self.settings.is_pinned(&app_id)
                || self
                    .identities
                    .get(&app_id)
                    .is_some_and(|i| i.launch.is_some());
            return if launchable {
                ItemAction::Launch(app_id)
            } else {
                ItemAction::None
            };
        }
        let active_pos = self
            .foreground
            .and_then(|fg| windows.iter().position(|w| *w == fg));
        match active_pos {
            Some(pos) if windows.len() == 1 => {
                if self.settings.behavior.click_active_minimizes {
                    ItemAction::MinimizeWindow(windows[pos])
                } else {
                    ItemAction::ActivateWindow(windows[pos])
                }
            }
            Some(pos) => ItemAction::ActivateWindow(windows[(pos + 1) % windows.len()]),
            None => ItemAction::ActivateWindow(windows[0]),
        }
    }
}

/// The facts of a window the overlap rule reads (`overlap::covers`): when an
/// update leaves them as they were, `overlapped` cannot have changed.
fn overlap_facts(raw: &RawWindow) -> (bool, bool, bool, u32, Option<Rect>) {
    (
        raw.is_visible,
        raw.is_minimized,
        raw.is_cloaked,
        raw.process_id,
        raw.rect,
    )
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::model::{LaunchTarget, MonitorId, Rect};
    use crate::settings::PinnedApp;

    fn win(id: u64, exe: &str, title: &str) -> RawWindow {
        RawWindow {
            id: WindowId(id),
            title: title.into(),
            class_name: "Window".into(),
            process_id: id as u32,
            exe_path: Some(PathBuf::from(exe)),
            app_user_model_id: None,
            is_visible: true,
            is_minimized: false,
            is_cloaked: false,
            on_current_desktop: None,
            monitor: None,
            rect: None,
            icon_key: None,
        }
    }

    fn cloaked(mut raw: RawWindow) -> RawWindow {
        raw.is_cloaked = true;
        raw
    }

    /// The platform could ask the shell: the window is (not) on the
    /// current virtual desktop.
    fn on_desktop(mut raw: RawWindow, on_current: bool) -> RawWindow {
        raw.on_current_desktop = Some(on_current);
        raw
    }

    fn pin(exe: &str) -> PinnedApp {
        PinnedApp {
            id: AppId::for_exe(Path::new(exe)),
            label: None,
            launch: LaunchTarget::Exe {
                path: PathBuf::from(exe),
                args: vec![],
                working_dir: None,
            },
        }
    }

    fn exe_id(exe: &str) -> AppId {
        AppId::for_exe(Path::new(exe))
    }

    fn identity(id: &AppId, name: &str, icon: &str) -> AppIdentity {
        AppIdentity {
            id: id.clone(),
            display_name: name.into(),
            icon_key: Some(IconKey(icon.into())),
            launch: None,
        }
    }

    fn keys(state: &DockState) -> Vec<&str> {
        state.items.iter().map(|i| i.key.as_str()).collect()
    }

    /// The primary monitor's dock window, whose monitor the single-dock
    /// tests never register: its clicks consider every window.
    const DOCK: &str = "dock";

    fn minimal_settings() -> Settings {
        let mut s = Settings::default();
        s.items.launcher = false;
        s.items.recycle_bin = false;
        s
    }

    fn added(raw: RawWindow) -> EngineInput {
        PlatformEvent::WindowAdded(raw).into()
    }

    fn updated(raw: RawWindow) -> EngineInput {
        PlatformEvent::WindowUpdated(raw).into()
    }

    fn removed(id: u64) -> EngineInput {
        PlatformEvent::WindowRemoved(WindowId(id)).into()
    }

    fn foreground(id: Option<u64>) -> EngineInput {
        PlatformEvent::ForegroundChanged(id.map(WindowId)).into()
    }

    fn attention(id: u64, flashing: bool) -> EngineInput {
        PlatformEvent::WindowAttention {
            id: WindowId(id),
            flashing,
        }
        .into()
    }

    // ----- the fullscreen verdict, per dock -------------------------------------

    /// The primary demo monitor and a second one to its right.
    const PRIMARY: &str = "mock-primary";
    const SECOND: &str = r"\\.\DISPLAY3";

    fn monitor(id: &str, primary: bool, x: i32) -> MonitorInfo {
        MonitorInfo {
            id: MonitorId(id.into()),
            name: id.into(),
            bounds: Rect::new(x, 0, 2560, 1440),
            work_area: Rect::new(x, 0, 2560, 1392),
            scale_factor: 1.0,
            is_primary: primary,
        }
    }

    fn monitors(list: Vec<MonitorInfo>) -> EngineInput {
        PlatformEvent::MonitorsChanged(list).into()
    }

    /// The platform's verdict, with the fullscreen window's monitor.
    fn fullscreen_on(monitor: Option<&str>) -> EngineInput {
        PlatformEvent::FullscreenChanged {
            active: true,
            monitor: monitor.map(|m| MonitorId(m.into())),
        }
        .into()
    }

    fn fullscreen_off() -> EngineInput {
        PlatformEvent::FullscreenChanged {
            active: false,
            monitor: None,
        }
        .into()
    }

    /// `DockState::docks` entry of `monitor`, if the snapshot has one.
    fn facts(state: &DockState, monitor: &str) -> Option<DockFacts> {
        state.docks.iter().find(|d| d.monitor.0 == monitor).cloned()
    }

    fn is_fullscreen(engine: &DockEngine, monitor: &str) -> bool {
        facts(&engine.snapshot(), monitor).is_some_and(|d| d.fullscreen)
    }

    fn is_overlapped(engine: &DockEngine, monitor: &str) -> bool {
        facts(&engine.snapshot(), monitor).is_some_and(|d| d.overlapped)
    }

    /// An engine that knows the primary monitor and has the primary dock
    /// window registered on it, without a panel rect yet.
    fn engine_with_primary_dock() -> DockEngine {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(monitors(vec![monitor(PRIMARY, true, 0)]));
        engine.apply(panel_of("dock", Some(PRIMARY), None));
        engine
    }

    #[test]
    fn fullscreen_is_stored_and_bumps_only_on_a_change() {
        let mut engine = engine_with_primary_dock();
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        let r = engine.snapshot().revision;
        assert!(!is_fullscreen(&engine, PRIMARY));
        assert!(!engine.fullscreen());
        assert_eq!(engine.fullscreen_monitor(), None);

        // The platform's first verdict is usually `false`: nothing to emit.
        assert!(!engine.apply(fullscreen_off()).changed);
        assert_eq!(engine.snapshot().revision, r);

        // A verdict that names the dock's monitor.
        let out = engine.apply(fullscreen_on(Some(PRIMARY)));
        assert!(out.changed);
        assert!(out.unresolved_apps.is_empty());
        let state = engine.snapshot();
        assert_eq!(
            state.docks,
            vec![DockFacts {
                monitor: MonitorId(PRIMARY.into()),
                fullscreen: true,
                overlapped: false,
            }]
        );
        assert!(engine.fullscreen());
        assert_eq!(
            engine.fullscreen_monitor(),
            Some(&MonitorId(PRIMARY.into()))
        );
        assert_eq!(state.revision, r + 1);
        // Items and foreground are untouched by it.
        assert_eq!(keys(&state), vec!["app:exe:c:/a.exe"]);

        // Repeats change nothing; the end of fullscreen is one more bump.
        assert!(!engine.apply(fullscreen_on(Some(PRIMARY))).changed);
        assert_eq!(engine.snapshot().revision, r + 1);
        assert!(engine.apply(fullscreen_off()).changed);
        assert!(!is_fullscreen(&engine, PRIMARY));
        assert!(!engine.fullscreen());
        assert_eq!(engine.fullscreen_monitor(), None);
        assert_eq!(engine.snapshot().revision, r + 2);

        // A verdict that names no monitor lands on the primary's dock.
        assert!(engine.apply(fullscreen_on(None)).changed);
        assert!(is_fullscreen(&engine, PRIMARY));
        assert_eq!(engine.fullscreen_monitor(), None);
    }

    #[test]
    fn fullscreen_survives_a_resync_and_is_independent_of_the_foreground() {
        let mut engine = engine_with_primary_dock();
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(fullscreen_on(Some(PRIMARY)));
        engine.apply(foreground(Some(1)));
        engine.apply(PlatformEvent::WindowsSnapshot(vec![win(1, r"C:\a.exe", "A")]).into());
        engine.apply(foreground(None));
        engine.apply(removed(1));
        // Only the platform says when it is over.
        assert!(is_fullscreen(&engine, PRIMARY));
    }

    #[test]
    fn fullscreen_lands_on_the_dock_of_the_verdicts_monitor_only() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(monitors(vec![
            monitor(PRIMARY, true, 0),
            monitor(SECOND, false, 2560),
        ]));
        engine.apply(panel_of("dock", Some(PRIMARY), None));
        engine.apply(panel_of("dock-DISPLAY3", Some(SECOND), None));
        let r = engine.snapshot().revision;

        // A video goes fullscreen on the second monitor: only its dock hides.
        assert!(engine.apply(fullscreen_on(Some(SECOND))).changed);
        assert!(!is_fullscreen(&engine, PRIMARY));
        assert!(is_fullscreen(&engine, SECOND));
        assert_eq!(engine.snapshot().revision, r + 1);

        // The fullscreen window moves to the primary: the fact moves with it.
        assert!(engine.apply(fullscreen_on(Some(PRIMARY))).changed);
        assert!(is_fullscreen(&engine, PRIMARY));
        assert!(!is_fullscreen(&engine, SECOND));

        // A verdict with no monitor: the primary's dock, not the other.
        assert!(!engine.apply(fullscreen_on(None)).changed);
        assert!(is_fullscreen(&engine, PRIMARY));
        assert!(!is_fullscreen(&engine, SECOND));

        // A monitor no dock sits on: nobody hides.
        assert!(engine.apply(fullscreen_on(Some(r"\\.\DISPLAY9"))).changed);
        assert!(!is_fullscreen(&engine, PRIMARY));
        assert!(!is_fullscreen(&engine, SECOND));
        assert!(engine.fullscreen());

        assert!(!engine.apply(fullscreen_off()).changed);
        assert!(!engine.fullscreen());
    }

    #[test]
    fn without_a_flagged_primary_the_first_monitor_takes_an_unplaced_verdict() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(monitors(vec![
            monitor(SECOND, false, 2560),
            monitor(PRIMARY, false, 0),
        ]));
        engine.apply(panel_of("dock", Some(SECOND), None));
        engine.apply(panel_of("dock-mock-primary", Some(PRIMARY), None));
        engine.apply(fullscreen_on(None));
        assert!(is_fullscreen(&engine, SECOND));
        assert!(!is_fullscreen(&engine, PRIMARY));
        // No monitors known at all: an unplaced verdict hides nobody.
        engine.apply(monitors(vec![]));
        assert!(!is_fullscreen(&engine, SECOND));
        assert!(!is_fullscreen(&engine, PRIMARY));
    }

    #[test]
    fn a_rect_change_alone_is_stored_without_bumping_the_revision() {
        let mut engine = engine_with_primary_dock();
        let mut raw = win(1, r"C:\a.exe", "A");
        raw.rect = Some(Rect::new(0, 0, 800, 600));
        engine.apply(added(raw.clone()));
        let r = engine.snapshot().revision;
        assert_eq!(
            engine.window(WindowId(1)).and_then(|w| w.rect),
            Some(Rect::new(0, 0, 800, 600))
        );

        // The window moved (the next query of it carried the new frame).
        raw.rect = Some(Rect::new(100, 50, 800, 600));
        assert!(!engine.apply(updated(raw.clone())).changed);
        assert_eq!(engine.snapshot().revision, r);
        assert_eq!(
            engine.window(WindowId(1)).and_then(|w| w.rect),
            Some(Rect::new(100, 50, 800, 600))
        );

        // A platform that cannot read the frame reports `None`; still no bump.
        raw.rect = None;
        assert!(!engine.apply(updated(raw.clone())).changed);
        assert_eq!(engine.window(WindowId(1)).and_then(|w| w.rect), None);
        assert_eq!(engine.snapshot().revision, r);

        // With a panel known, a move that stays clear of it is still
        // stored without a bump.
        assert!(!engine.apply(panel(Some(PANEL))).changed);
        raw.rect = Some(Rect::new(0, 0, 800, 600));
        assert!(!engine.apply(updated(raw)).changed);
        assert_eq!(engine.snapshot().revision, r);
        assert!(!engine.overlapped("dock"));
    }

    // ----- a window over the dock ------------------------------------------------

    /// A 560 × 72 panel at the bottom of a 2560 × 1392 work area.
    const PANEL: Rect = Rect::new(1000, 1320, 560, 72);
    /// A frame that covers the panel's left half.
    const OVER: Rect = Rect::new(600, 900, 700, 480);
    /// A frame in the top-left corner, clear of the panel.
    const CLEAR: Rect = Rect::new(0, 0, 800, 600);

    /// The app layer's report about one dock window.
    fn panel_of(label: &str, monitor: Option<&str>, rect: Option<Rect>) -> EngineInput {
        EngineInput::DockPanelChanged {
            label: label.to_owned(),
            monitor: monitor.map(|m| MonitorId(m.into())),
            rect,
        }
    }

    /// The primary dock window's panel, as the single-dock tests report it.
    fn panel(rect: Option<Rect>) -> EngineInput {
        panel_of("dock", Some(PRIMARY), rect)
    }

    fn framed(id: u64, exe: &str, rect: Rect) -> RawWindow {
        let mut raw = win(id, exe, "W");
        raw.rect = Some(rect);
        raw
    }

    #[test]
    fn overlapped_follows_the_panel_and_the_windows_and_bumps_only_when_it_flips() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(framed(1, r"C:\a.exe", OVER)));
        let r = engine.snapshot().revision;
        // No panel yet: nothing can overlap it, and there is no dock entry.
        assert!(!engine.overlapped("dock"));
        assert!(engine.snapshot().docks.is_empty());
        assert_eq!(engine.dock_panel("dock"), None);
        assert_eq!(engine.dock_monitor("dock"), None);

        // The UI's first report: the window is over it. The dock gets its
        // entry (one bump) with the flag set.
        let out = engine.apply(panel(Some(PANEL)));
        assert!(out.changed);
        assert!(out.unresolved_apps.is_empty());
        assert!(engine.overlapped("dock"));
        assert_eq!(engine.dock_panel("dock"), Some(PANEL));
        assert_eq!(
            engine.dock_monitor("dock"),
            Some(&MonitorId(PRIMARY.into()))
        );
        let state = engine.snapshot();
        assert_eq!(
            state.docks,
            vec![DockFacts {
                monitor: MonitorId(PRIMARY.into()),
                fullscreen: false,
                overlapped: true,
            }]
        );
        assert_eq!(state.revision, r + 1);
        assert_eq!(keys(&state), vec!["app:exe:c:/a.exe"]);

        // The same panel again, and a move that keeps covering it: no bump.
        assert!(!engine.apply(panel(Some(PANEL))).changed);
        let mut moved = framed(1, r"C:\a.exe", Rect::new(650, 950, 700, 480));
        assert!(!engine.apply(updated(moved.clone())).changed);
        assert_eq!(engine.snapshot().revision, r + 1);

        // Moved clear of it: the flag flips, one bump.
        moved.rect = Some(CLEAR);
        assert!(engine.apply(updated(moved.clone())).changed);
        assert!(!is_overlapped(&engine, PRIMARY));
        assert_eq!(engine.snapshot().revision, r + 2);

        // Back over it, then minimised, restored, cloaked, uncloaked, hidden.
        moved.rect = Some(OVER);
        assert!(engine.apply(updated(moved.clone())).changed);
        assert!(is_overlapped(&engine, PRIMARY));
        moved.is_minimized = true;
        assert!(engine.apply(updated(moved.clone())).changed);
        assert!(!is_overlapped(&engine, PRIMARY));
        moved.is_minimized = false;
        assert!(engine.apply(updated(moved.clone())).changed);
        assert!(is_overlapped(&engine, PRIMARY));
        moved.is_cloaked = true;
        assert!(engine.apply(updated(moved.clone())).changed);
        assert!(!is_overlapped(&engine, PRIMARY));
        moved.is_cloaked = false;
        assert!(engine.apply(updated(moved.clone())).changed);
        assert!(is_overlapped(&engine, PRIMARY));
        moved.is_visible = false;
        assert!(engine.apply(updated(moved.clone())).changed);
        assert!(!is_overlapped(&engine, PRIMARY));
        moved.is_visible = true;
        assert!(engine.apply(updated(moved)).changed);
        assert!(is_overlapped(&engine, PRIMARY));

        // The window closes: clear. A new window over it: set again.
        assert!(engine.apply(removed(1)).changed);
        assert!(!is_overlapped(&engine, PRIMARY));
        assert!(engine.apply(added(framed(2, r"C:\b.exe", OVER))).changed);
        assert!(is_overlapped(&engine, PRIMARY));

        // The dock window was hidden: no panel, nothing overlaps; the dock
        // keeps its entry (it is still on its monitor).
        let r = engine.snapshot().revision;
        assert!(engine.apply(panel(None)).changed);
        assert!(!is_overlapped(&engine, PRIMARY));
        assert!(facts(&engine.snapshot(), PRIMARY).is_some());
        assert_eq!(engine.dock_panel("dock"), None);
        assert_eq!(engine.snapshot().revision, r + 1);
        // Shown again where it was: set again.
        assert!(engine.apply(panel(Some(PANEL))).changed);
        assert!(is_overlapped(&engine, PRIMARY));
    }

    #[test]
    fn a_panel_change_that_does_not_flip_overlapped_does_not_bump() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(framed(1, r"C:\a.exe", CLEAR)));
        engine.apply(panel(Some(PANEL)));
        let r = engine.snapshot().revision;
        // The dock grew by an item: a wider panel, still clear of the window.
        let wider = Rect::new(972, 1320, 616, 72);
        assert!(!engine.apply(panel(Some(wider))).changed);
        assert_eq!(engine.dock_panel("dock"), Some(wider));
        assert_eq!(engine.snapshot().revision, r);
        assert!(!engine.apply(panel(None)).changed);
        assert_eq!(engine.snapshot().revision, r);
    }

    #[test]
    fn overlap_ignores_cloaked_windows_whatever_show_only_current_desktop_says() {
        let mut settings = minimal_settings();
        settings.behavior.show_only_current_desktop = false;
        let mut engine = DockEngine::new(settings);
        engine.apply(added(cloaked(framed(1, r"C:\a.exe", OVER))));
        engine.apply(panel(Some(PANEL)));
        // The item is shown (the setting lists other desktops' windows) ...
        assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/a.exe"]);
        // ... but a cloaked window is not on screen, so it covers nothing.
        assert!(!is_overlapped(&engine, PRIMARY));
        assert!(engine.apply(updated(framed(1, r"C:\a.exe", OVER))).changed);
        assert!(is_overlapped(&engine, PRIMARY));
    }

    #[test]
    fn our_own_windows_never_overlap_the_dock() {
        let mut engine = DockEngine::new(minimal_settings());
        let mut ours = framed(1, r"C:\stem.exe", OVER);
        ours.process_id = 4242;
        engine.apply(added(ours.clone()));
        engine.apply(panel(Some(PANEL)));
        assert!(is_overlapped(&engine, PRIMARY));

        // The service tells the engine who we are: the flag clears with the
        // next input that touches the rule, without a bump of its own.
        engine.set_own_process_id(Some(4242));
        let out = engine.apply(updated(ours.clone()));
        assert!(out.changed);
        assert!(!is_overlapped(&engine, PRIMARY));
        // A window of another process over it still counts.
        let mut theirs = framed(2, r"C:\b.exe", OVER);
        theirs.process_id = 4243;
        assert!(engine.apply(added(theirs)).changed);
        assert!(is_overlapped(&engine, PRIMARY));
        assert!(engine.apply(removed(2)).changed);
        assert!(!is_overlapped(&engine, PRIMARY));
        // Forgetting the id makes ours count again.
        engine.set_own_process_id(None);
        assert!(engine.apply(updated(ours)).changed);
        assert!(is_overlapped(&engine, PRIMARY));
    }

    #[test]
    fn a_resync_recomputes_overlap() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(panel(Some(PANEL)));
        engine.apply(added(framed(1, r"C:\a.exe", CLEAR)));
        assert!(!is_overlapped(&engine, PRIMARY));

        // The platform re-enumerates: the window has moved over the dock.
        let out =
            engine.apply(PlatformEvent::WindowsSnapshot(vec![framed(1, r"C:\a.exe", OVER)]).into());
        assert!(out.changed);
        assert!(is_overlapped(&engine, PRIMARY));
        // A resync that lists nothing clears it.
        assert!(
            engine
                .apply(PlatformEvent::WindowsSnapshot(vec![]).into())
                .changed
        );
        assert!(!is_overlapped(&engine, PRIMARY));
        // Identical resyncs are no-ops.
        engine.apply(PlatformEvent::WindowsSnapshot(vec![framed(1, r"C:\a.exe", OVER)]).into());
        let r = engine.snapshot().revision;
        assert!(
            !engine
                .apply(PlatformEvent::WindowsSnapshot(vec![framed(1, r"C:\a.exe", OVER)]).into())
                .changed
        );
        assert_eq!(engine.snapshot().revision, r);
    }

    #[test]
    fn overlap_is_independent_of_fullscreen_the_foreground_and_the_settings() {
        let mut settings = minimal_settings();
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(monitors(vec![monitor(PRIMARY, true, 0)]));
        engine.apply(added(framed(1, r"C:\a.exe", OVER)));
        engine.apply(panel(Some(PANEL)));
        assert!(is_overlapped(&engine, PRIMARY));

        engine.apply(foreground(Some(1)));
        assert!(is_overlapped(&engine, PRIMARY));
        let state = engine.snapshot();
        engine.apply(fullscreen_on(Some(PRIMARY)));
        assert!(is_overlapped(&engine, PRIMARY));
        assert!(is_fullscreen(&engine, PRIMARY));
        engine.apply(fullscreen_off());
        assert!(is_overlapped(&engine, PRIMARY));
        assert!(!is_fullscreen(&engine, PRIMARY));
        settings.icon_size = 64;
        settings.behavior.show_only_current_desktop = false;
        assert!(!engine.apply(EngineInput::SettingsChanged(settings)).changed);
        assert!(is_overlapped(&engine, PRIMARY));
        assert_eq!(engine.snapshot().items, state.items);
    }

    // ----- one dock per monitor ------------------------------------------------

    /// A 560 × 72 panel at the bottom of the second monitor's work area.
    const SECOND_PANEL: Rect = Rect::new(3560, 1320, 560, 72);
    /// A frame on the second monitor that covers that panel's left half.
    const SECOND_OVER: Rect = Rect::new(3160, 900, 700, 480);

    #[test]
    fn each_dock_is_overlapped_by_the_windows_over_its_own_panel() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(monitors(vec![
            monitor(PRIMARY, true, 0),
            monitor(SECOND, false, 2560),
        ]));
        engine.apply(panel_of("dock", Some(PRIMARY), Some(PANEL)));
        engine.apply(panel_of("dock-DISPLAY3", Some(SECOND), Some(SECOND_PANEL)));
        // Two entries, sorted by monitor id (`\\.\DISPLAY3` < `mock-primary`).
        let state = engine.snapshot();
        assert_eq!(
            state
                .docks
                .iter()
                .map(|d| d.monitor.0.as_str())
                .collect::<Vec<_>>(),
            vec![SECOND, PRIMARY]
        );
        assert!(state.docks.iter().all(|d| !d.overlapped && !d.fullscreen));
        let r = state.revision;

        // A window over the second dock's panel: only that dock.
        assert!(
            engine
                .apply(added(framed(1, r"C:\a.exe", SECOND_OVER)))
                .changed
        );
        assert!(!is_overlapped(&engine, PRIMARY));
        assert!(is_overlapped(&engine, SECOND));
        assert!(engine.overlapped("dock-DISPLAY3"));
        assert!(!engine.overlapped("dock"));
        // One over the primary's as well: both.
        let r2 = engine.snapshot().revision;
        assert!(engine.apply(added(framed(2, r"C:\b.exe", OVER))).changed);
        assert!(is_overlapped(&engine, PRIMARY));
        assert!(is_overlapped(&engine, SECOND));
        assert!(r2 > r);
        // The first one dragged from the second monitor to the primary,
        // over the primary's panel: the second clears, the primary stays.
        assert!(engine.apply(updated(framed(1, r"C:\a.exe", OVER))).changed);
        assert!(is_overlapped(&engine, PRIMARY));
        assert!(!is_overlapped(&engine, SECOND));

        // Dragged back under the second dock: that flag is set again (the
        // primary's stays, window 2 is still there).
        assert!(
            engine
                .apply(updated(framed(1, r"C:\a.exe", SECOND_OVER)))
                .changed
        );
        assert!(is_overlapped(&engine, PRIMARY));
        assert!(is_overlapped(&engine, SECOND));

        // The second dock's window is hidden by the tray: its flag clears,
        // its entry stays; shown again: back.
        assert!(
            engine
                .apply(panel_of("dock-DISPLAY3", Some(SECOND), None))
                .changed
        );
        assert!(!is_overlapped(&engine, SECOND));
        assert!(facts(&engine.snapshot(), SECOND).is_some());
        assert!(
            engine
                .apply(panel_of("dock-DISPLAY3", Some(SECOND), Some(SECOND_PANEL)))
                .changed
        );
        assert!(is_overlapped(&engine, SECOND));
    }

    #[test]
    fn docks_come_and_go_with_their_windows_and_only_that_bumps() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(monitors(vec![
            monitor(PRIMARY, true, 0),
            monitor(SECOND, false, 2560),
        ]));
        engine.apply(panel_of("dock", Some(PRIMARY), Some(PANEL)));
        let r = engine.snapshot().revision;
        assert_eq!(engine.snapshot().docks.len(), 1);

        // A second monitor's dock is created: one new entry, one bump.
        assert!(
            engine
                .apply(panel_of("dock-DISPLAY3", Some(SECOND), None))
                .changed
        );
        assert_eq!(engine.snapshot().docks.len(), 2);
        assert_eq!(engine.snapshot().revision, r + 1);
        // Its page reports a panel clear of every window: no bump.
        assert!(
            !engine
                .apply(panel_of("dock-DISPLAY3", Some(SECOND), Some(SECOND_PANEL)))
                .changed
        );
        assert_eq!(engine.dock_panel("dock-DISPLAY3"), Some(SECOND_PANEL));
        assert_eq!(engine.snapshot().revision, r + 1);

        // The monitor is unplugged and its window closed: the entry goes,
        // one bump; the label is forgotten.
        assert!(engine.apply(panel_of("dock-DISPLAY3", None, None)).changed);
        assert_eq!(engine.snapshot().docks.len(), 1);
        assert_eq!(engine.snapshot().revision, r + 2);
        assert_eq!(engine.dock_panel("dock-DISPLAY3"), None);
        assert_eq!(engine.dock_monitor("dock-DISPLAY3"), None);
        assert!(!engine.overlapped("dock-DISPLAY3"));
        // Forgetting it again is a no-op.
        assert!(!engine.apply(panel_of("dock-DISPLAY3", None, None)).changed);
        assert_eq!(engine.snapshot().revision, r + 2);
    }

    #[test]
    fn a_dock_whose_monitor_is_unknown_has_no_entry_until_it_is_placed() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(monitors(vec![monitor(PRIMARY, true, 0)]));
        engine.apply(added(framed(1, r"C:\a.exe", OVER)));
        let r = engine.snapshot().revision;

        // The app layer had no monitor list when it placed the window: the
        // panel is remembered, but nothing is published for it.
        assert!(!engine.apply(panel_of("dock", None, Some(PANEL))).changed);
        assert!(engine.snapshot().docks.is_empty());
        assert_eq!(engine.dock_panel("dock"), Some(PANEL));
        assert!(
            engine.overlapped("dock"),
            "the flag is kept up to date meanwhile"
        );
        assert_eq!(engine.snapshot().revision, r);
        // A fullscreen verdict changes nothing visible either.
        assert!(!engine.apply(fullscreen_on(None)).changed);

        // The monitors arrive and the window is placed: the entry appears
        // with both facts right, in one bump.
        assert!(
            engine
                .apply(panel_of("dock", Some(PRIMARY), Some(PANEL)))
                .changed
        );
        assert_eq!(
            engine.snapshot().docks,
            vec![DockFacts {
                monitor: MonitorId(PRIMARY.into()),
                fullscreen: true,
                overlapped: true,
            }]
        );
        assert_eq!(engine.snapshot().revision, r + 1);
    }

    #[test]
    fn two_docks_on_one_monitor_fold_into_one_entry() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(monitors(vec![monitor(PRIMARY, true, 0)]));
        engine.apply(added(framed(1, r"C:\a.exe", OVER)));
        engine.apply(panel_of("dock", Some(PRIMARY), Some(CLEAR)));
        engine.apply(panel_of("dock-extra", Some(PRIMARY), Some(PANEL)));
        let state = engine.snapshot();
        assert_eq!(state.docks.len(), 1);
        assert!(state.docks[0].overlapped, "any panel of the monitor counts");
        assert!(!engine.overlapped("dock"));
        assert!(engine.overlapped("dock-extra"));
    }

    #[test]
    fn a_docks_monitor_can_change_without_its_panel_changing() {
        // The primary monitor changed in Windows Settings: the `dock` window
        // moves to the new primary and the app layer re-registers it there.
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(monitors(vec![
            monitor(PRIMARY, true, 0),
            monitor(SECOND, false, 2560),
        ]));
        engine.apply(panel_of("dock", Some(PRIMARY), Some(PANEL)));
        engine.apply(fullscreen_on(None));
        assert!(is_fullscreen(&engine, PRIMARY));
        let r = engine.snapshot().revision;

        engine.apply(monitors(vec![
            monitor(PRIMARY, false, 0),
            monitor(SECOND, true, 2560),
        ]));
        // The verdict without a monitor now points at the second monitor,
        // where no dock is yet: the primary's dock clears (one bump).
        assert!(!is_fullscreen(&engine, PRIMARY));
        assert_eq!(engine.snapshot().revision, r + 1);
        assert!(
            engine
                .apply(panel_of("dock", Some(SECOND), Some(SECOND_PANEL)))
                .changed
        );
        assert_eq!(engine.dock_monitor("dock"), Some(&MonitorId(SECOND.into())));
        assert!(is_fullscreen(&engine, SECOND));
        assert!(facts(&engine.snapshot(), PRIMARY).is_none());
    }

    #[test]
    fn empty_engine_shows_only_fixed_items() {
        let engine = DockEngine::new(Settings::default());
        let state = engine.snapshot();
        assert_eq!(
            keys(&state),
            vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
        );
    }

    #[test]
    fn windows_are_grouped_by_app_and_ordered_by_first_seen() {
        let mut engine = DockEngine::new(minimal_settings());
        let out = engine.apply(added(win(1, r"C:\b.exe", "B1")));
        assert!(out.changed);
        assert_eq!(out.unresolved_apps, vec![exe_id(r"C:\b.exe")]);
        engine.apply(added(win(2, r"C:\a.exe", "A1")));
        engine.apply(added(win(3, r"C:\b.exe", "B2")));

        let state = engine.snapshot();
        assert_eq!(keys(&state), vec!["app:exe:c:/b.exe", "app:exe:c:/a.exe"]);
        assert_eq!(state.items[0].windows.len(), 2);
        assert_eq!(state.items[0].title, "B1");
    }

    #[test]
    fn pinned_apps_come_first_and_absorb_their_windows() {
        let mut settings = minimal_settings();
        settings.pinned = vec![pin(r"C:\a.exe"), pin(r"C:\z.exe")];
        let mut engine = DockEngine::new(settings);
        engine.apply(added(win(1, r"C:\b.exe", "B")));
        engine.apply(added(win(2, r"C:\a.exe", "A")));

        let state = engine.snapshot();
        assert_eq!(
            keys(&state),
            vec![
                "app:exe:c:/a.exe",
                "app:exe:c:/z.exe",
                "sep:1",
                "app:exe:c:/b.exe"
            ]
        );
        assert!(state.items[0].pinned);
        assert_eq!(state.items[0].windows.len(), 1);
        assert!(state.items[1].windows.is_empty());
        assert!(!state.items[3].pinned);
    }

    #[test]
    fn foreground_marks_active_and_clears_attention() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(attention(1, true));
        assert!(engine.snapshot().items[0].needs_attention);
        let out = engine.apply(foreground(Some(1)));
        assert!(out.changed);
        let item = &engine.snapshot().items[0];
        assert!(item.is_active);
        assert!(!item.needs_attention);
    }

    #[test]
    fn a_flash_of_the_foreground_window_does_not_ask_for_attention() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(added(win(2, r"C:\b.exe", "B")));
        engine.apply(foreground(Some(1)));
        let before = engine.snapshot();

        // The window in front flashes: nothing to show, nothing to clear later.
        assert!(!engine.apply(attention(1, true)).changed);
        assert_eq!(engine.snapshot(), before);

        // A window behind it still asks for attention ...
        assert!(engine.apply(attention(2, true)).changed);
        assert!(engine.snapshot().items[1].needs_attention);

        // ... and once the first window is no longer in front, so does it.
        engine.apply(foreground(Some(2)));
        assert!(engine.apply(attention(1, true)).changed);
        let state = engine.snapshot();
        assert!(state.items[0].needs_attention);
        assert!(!state.items[1].needs_attention);
    }

    #[test]
    fn cloaked_windows_are_hidden_by_default() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(cloaked(win(1, r"C:\a.exe", "A"))));
        assert!(engine.snapshot().items.is_empty());
    }

    #[test]
    fn snapshot_resync_preserves_order_and_drops_vanished() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(added(win(2, r"C:\b.exe", "B")));
        engine.apply(
            PlatformEvent::WindowsSnapshot(vec![
                win(2, r"C:\b.exe", "B"),
                win(3, r"C:\c.exe", "C"),
            ])
            .into(),
        );
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["app:exe:c:/b.exe", "app:exe:c:/c.exe"]
        );
    }

    #[test]
    fn primary_action_semantics() {
        let mut settings = minimal_settings();
        settings.pinned = vec![pin(r"C:\p.exe")];
        let mut engine = DockEngine::new(settings);
        let p = exe_id(r"C:\p.exe");

        // Pinned, not running -> launch.
        assert_eq!(
            engine.primary_action("app:exe:c:/p.exe", DOCK),
            ItemAction::Launch(p)
        );

        // One window, not active -> activate.
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::ActivateWindow(WindowId(1))
        );

        // One window, active -> minimise.
        engine.apply(foreground(Some(1)));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::MinimizeWindow(WindowId(1))
        );

        // Two windows, first active -> cycle to the second.
        engine.apply(added(win(2, r"C:\a.exe", "A2")));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::ActivateWindow(WindowId(2))
        );

        // Unknown running app with no launch info -> nothing after it closes.
        engine.apply(removed(1));
        engine.apply(removed(2));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::None
        );

        // Fixed items.
        assert_eq!(
            engine.primary_action("fixed:launcher", DOCK),
            ItemAction::Shell(ShellAction::OpenStartMenu)
        );
        assert_eq!(engine.primary_action("nonsense", DOCK), ItemAction::None);
    }

    #[test]
    fn primary_action_keeps_the_active_window_when_click_active_minimizes_is_off() {
        let mut settings = minimal_settings();
        settings.behavior.click_active_minimizes = false;
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(foreground(Some(1)));

        // One window, active -> activated again, not minimised.
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::ActivateWindow(WindowId(1))
        );

        // Several windows still cycle, as with the setting on.
        engine.apply(added(win(2, r"C:\a.exe", "A2")));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::ActivateWindow(WindowId(2))
        );

        // The setting is read at click time: turning it on minimises again.
        engine.apply(removed(2));
        settings.behavior.click_active_minimizes = true;
        engine.apply(EngineInput::SettingsChanged(settings));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::MinimizeWindow(WindowId(1))
        );
    }

    #[test]
    fn identity_is_requested_once_until_resolved() {
        let mut engine = DockEngine::new(minimal_settings());
        let a = exe_id(r"C:\a.exe");
        let out = engine.apply(added(win(1, r"C:\a.exe", "A")));
        assert_eq!(out.unresolved_apps, vec![a.clone()]);
        let out = engine.apply(added(win(2, r"C:\a.exe", "A2")));
        assert!(out.unresolved_apps.is_empty());
        assert_eq!(engine.unresolved_apps(), vec![a.clone()]);

        let out = engine.apply(EngineInput::IdentityResolved {
            app_id: a.clone(),
            identity: identity(&a, "App A", "k"),
        });
        assert!(out.changed);
        assert!(engine.unresolved_apps().is_empty());
        let item = &engine.snapshot().items[0];
        assert_eq!(item.title, "App A");
        assert_eq!(item.icon_key, Some(IconKey("k".into())));
    }

    #[test]
    fn revision_only_advances_on_change() {
        let mut engine = DockEngine::new(minimal_settings());
        let r0 = engine.snapshot().revision;
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        let r1 = engine.snapshot().revision;
        assert!(r1 > r0);
        let out = engine.apply(updated(win(1, r"C:\a.exe", "A")));
        assert!(!out.changed);
        assert_eq!(engine.snapshot().revision, r1);
    }

    // ----- edge cases --------------------------------------------------------

    #[test]
    fn window_regroups_when_app_user_model_id_appears_later() {
        let mut engine = DockEngine::new(minimal_settings());
        let exe = exe_id(r"C:\app.exe");
        let aumid = AppId::for_app_user_model_id("Vendor.App_abc!App");

        engine.apply(added(win(1, r"C:\app.exe", "First")));
        engine.apply(added(win(2, r"C:\app.exe", "Second")));
        engine.apply(attention(1, true));
        assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/app.exe"]);

        // The platform learns the AUMID of window 1 only now.
        let mut moved = win(1, r"C:\app.exe", "First");
        moved.app_user_model_id = Some("Vendor.App_abc!App".into());
        let out = engine.apply(updated(moved));
        assert!(out.changed);
        assert_eq!(out.unresolved_apps, vec![aumid.clone()]);

        let state = engine.snapshot();
        // The exe item keeps its first-seen slot; the AUMID item is a newcomer.
        assert_eq!(
            keys(&state),
            vec!["app:exe:c:/app.exe", "app:aumid:Vendor.App_abc!App"]
        );
        assert_eq!(state.items[0].windows.len(), 1);
        assert_eq!(state.items[1].windows.len(), 1);
        // Attention travels with the window.
        assert!(!state.items[0].needs_attention);
        assert!(state.items[1].needs_attention);
        assert_eq!(engine.windows_of(&aumid), vec![WindowId(1)]);
        assert_eq!(engine.windows_of(&exe), vec![WindowId(2)]);
        // Both apps are still unresolved (sorted by id); only the new one was
        // requested by this input, the exe id was requested when first seen.
        assert_eq!(engine.unresolved_apps(), vec![aumid, exe]);
    }

    #[test]
    fn pinned_launch_target_change_keeps_key_identity_and_revision() {
        let mut settings = minimal_settings();
        settings.pinned = vec![pin(r"C:\a.exe")];
        let a = exe_id(r"C:\a.exe");
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(EngineInput::IdentityResolved {
            app_id: a.clone(),
            identity: identity(&a, "App A", "icon-a"),
        });
        let before = engine.snapshot();

        settings.pinned[0].launch = LaunchTarget::Shortcut {
            path: PathBuf::from(r"C:\Users\me\Desktop\A.lnk"),
        };
        let out = engine.apply(EngineInput::SettingsChanged(settings));
        // Nothing the UI renders changed, so no new revision and no new
        // identity request; the click still launches by id.
        assert!(!out.changed);
        assert!(out.unresolved_apps.is_empty());
        assert_eq!(engine.snapshot(), before);
        assert_eq!(engine.snapshot().items[0].title, "App A");
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::Launch(a.clone())
        );
        assert!(matches!(
            engine.settings().pinned[0].launch,
            LaunchTarget::Shortcut { .. }
        ));
        assert_eq!(
            engine.identity(&a).map(|i| i.display_name.as_str()),
            Some("App A")
        );
    }

    #[test]
    fn resync_after_many_adds_is_a_no_op() {
        let mut engine = DockEngine::new(minimal_settings());
        let windows: Vec<RawWindow> = (1..=40)
            .map(|i| win(i, &format!(r"C:\app{}.exe", i % 7), &format!("W{i}")))
            .collect();
        for w in &windows {
            engine.apply(added(w.clone()));
        }
        engine.apply(foreground(Some(5)));
        engine.apply(attention(9, true));
        let before = engine.snapshot();
        assert_eq!(before.items.len(), 7);

        // The platform re-enumerates in a different order.
        let mut list = windows.clone();
        list.reverse();
        let out = engine.apply(PlatformEvent::WindowsSnapshot(list).into());
        assert!(!out.changed);
        assert!(out.unresolved_apps.is_empty());
        assert_eq!(engine.snapshot(), before);
        assert_eq!(engine.foreground(), Some(WindowId(5)));

        // Window order inside an item also survives (first seen, not list order).
        let item = &engine.snapshot().items[0];
        let ids: Vec<u64> = item.windows.iter().map(|w| w.id.0).collect();
        assert_eq!(ids, vec![1, 8, 15, 22, 29, 36]);
    }

    #[test]
    fn resync_drops_a_vanished_foreground_window() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(added(win(2, r"C:\b.exe", "B")));
        engine.apply(foreground(Some(1)));
        assert!(engine.snapshot().items[0].is_active);

        let out =
            engine.apply(PlatformEvent::WindowsSnapshot(vec![win(2, r"C:\b.exe", "B")]).into());
        assert!(out.changed);
        assert_eq!(engine.foreground(), None);
        let state = engine.snapshot();
        assert_eq!(state.foreground, None);
        assert!(state.items.iter().all(|i| !i.is_active));
    }

    #[test]
    fn foreground_on_a_cloaked_window_marks_nothing_active_until_it_is_shown() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(cloaked(win(1, r"C:\a.exe", "A"))));
        engine.apply(added(win(2, r"C:\b.exe", "B")));
        engine.apply(foreground(Some(2)));

        // The user switches to the virtual desktop with window 1: the OS
        // reports it as foreground while it is still cloaked for us.
        let out = engine.apply(foreground(Some(1)));
        assert!(out.changed);
        let state = engine.snapshot();
        assert_eq!(state.foreground, Some(WindowId(1)));
        assert_eq!(keys(&state), vec!["app:exe:c:/b.exe"]);
        assert!(state.items.iter().all(|i| !i.is_active));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::None
        );

        // Uncloaking alone makes the item active; no new ForegroundChanged needed.
        let out = engine.apply(updated(win(1, r"C:\a.exe", "A")));
        assert!(out.changed);
        let state = engine.snapshot();
        assert_eq!(keys(&state), vec!["app:exe:c:/b.exe", "app:exe:c:/a.exe"]);
        assert!(state.items[1].is_active);
        assert!(!state.items[0].is_active);
    }

    #[test]
    fn show_only_current_desktop_false_shows_cloaked_windows_and_toggles_live() {
        let mut settings = minimal_settings();
        settings.behavior.show_only_current_desktop = false;
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(added(cloaked(win(1, r"C:\a.exe", "A"))));
        engine.apply(foreground(Some(1)));
        let state = engine.snapshot();
        assert_eq!(keys(&state), vec!["app:exe:c:/a.exe"]);
        assert!(state.items[0].is_active);
        assert_eq!(engine.windows_of(&exe_id(r"C:\a.exe")), vec![WindowId(1)]);
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::MinimizeWindow(WindowId(1))
        );

        settings.behavior.show_only_current_desktop = true;
        let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
        assert!(out.changed);
        assert!(engine.snapshot().items.is_empty());
        assert!(engine.windows_of(&exe_id(r"C:\a.exe")).is_empty());

        settings.behavior.show_only_current_desktop = false;
        let out = engine.apply(EngineInput::SettingsChanged(settings));
        assert!(out.changed);
        assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/a.exe"]);
    }

    #[test]
    fn on_current_desktop_decides_instead_of_the_cloak_state_when_known() {
        let mut settings = minimal_settings();
        settings.behavior.show_only_current_desktop = true;
        let mut engine = DockEngine::new(settings.clone());
        // Parked on another desktop: away, cloaked or not.
        engine.apply(added(on_desktop(win(1, r"C:\a.exe", "A"), false)));
        engine.apply(added(on_desktop(cloaked(win(2, r"C:\b.exe", "B")), false)));
        // On this desktop but cloaked: a suspended Store app, or a window in
        // the switch animation. Listed.
        engine.apply(added(on_desktop(cloaked(win(3, r"C:\c.exe", "C")), true)));
        // Unknown: the cloak state decides.
        engine.apply(added(cloaked(win(4, r"C:\d.exe", "D"))));
        engine.apply(added(win(5, r"C:\e.exe", "E")));
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["app:exe:c:/c.exe", "app:exe:c:/e.exe"]
        );
        assert!(engine.windows_of(&exe_id(r"C:\a.exe")).is_empty());
        assert_eq!(engine.windows_of(&exe_id(r"C:\c.exe")), vec![WindowId(3)]);
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::None
        );
        assert_eq!(
            engine.primary_action("app:exe:c:/c.exe", DOCK),
            ItemAction::ActivateWindow(WindowId(3))
        );

        // With the setting off every window is listed whatever the desktop:
        // the apps already listed keep their order, the others follow.
        settings.behavior.show_only_current_desktop = false;
        let out = engine.apply(EngineInput::SettingsChanged(settings));
        assert!(out.changed);
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "app:exe:c:/c.exe",
                "app:exe:c:/e.exe",
                "app:exe:c:/a.exe",
                "app:exe:c:/b.exe",
                "app:exe:c:/d.exe",
            ]
        );
    }

    #[test]
    fn a_desktop_switch_arrives_as_updates_and_then_as_a_snapshot() {
        let mut engine = DockEngine::new(minimal_settings());
        let here = |id, exe, title| on_desktop(win(id, exe, title), true);
        let away = |id, exe, title| on_desktop(cloaked(win(id, exe, title)), false);
        let snapshot =
            |list: Vec<RawWindow>| EngineInput::from(PlatformEvent::WindowsSnapshot(list));
        engine.apply(snapshot(vec![
            here(1, r"C:\a.exe", "A"),
            away(2, r"C:\b.exe", "B"),
        ]));
        engine.apply(foreground(Some(1)));
        let state = engine.snapshot();
        assert_eq!(keys(&state), vec!["app:exe:c:/a.exe"]);
        assert!(state.items[0].is_active);

        // Win+Ctrl+Right: the shell cloaks A and uncloaks B, one event each,
        // and B takes the foreground.
        let out = engine.apply(updated(away(1, r"C:\a.exe", "A")));
        assert!(out.changed);
        assert!(engine.snapshot().items.is_empty());
        let out = engine.apply(updated(here(2, r"C:\b.exe", "B")));
        assert!(out.changed);
        engine.apply(foreground(Some(2)));
        let state = engine.snapshot();
        assert_eq!(keys(&state), vec!["app:exe:c:/b.exe"]);
        assert!(state.items[0].is_active);

        // The platform's snapshot 400 ms later lists the same facts: a no-op.
        let revision = state.revision;
        let out = engine.apply(snapshot(vec![
            away(1, r"C:\a.exe", "A"),
            here(2, r"C:\b.exe", "B"),
        ]));
        assert!(!out.changed);
        assert_eq!(engine.snapshot().revision, revision);

        // A snapshot that reveals a window the events missed (C uncloaked on
        // this desktop) adds it, after B, which was seen first.
        let out = engine.apply(snapshot(vec![
            away(1, r"C:\a.exe", "A"),
            here(2, r"C:\b.exe", "B"),
            here(3, r"C:\c.exe", "C"),
        ]));
        assert!(out.changed);
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["app:exe:c:/b.exe", "app:exe:c:/c.exe"]
        );
    }

    #[test]
    fn attention_is_dropped_when_the_window_closes() {
        let mut settings = minimal_settings();
        settings.pinned = vec![pin(r"C:\p.exe")];
        let mut engine = DockEngine::new(settings);
        engine.apply(added(win(1, r"C:\p.exe", "P")));
        engine.apply(added(win(2, r"C:\u.exe", "U")));
        engine.apply(attention(1, true));
        engine.apply(attention(2, true));
        let state = engine.snapshot();
        assert!(
            state
                .items
                .iter()
                .all(|i| i.kind != DockItemKind::App || i.needs_attention)
        );

        let out = engine.apply(removed(1));
        assert!(out.changed);
        let out = engine.apply(removed(2));
        assert!(out.changed);
        let state = engine.snapshot();
        // Pinned item stays, calm; the unpinned one is gone.
        assert_eq!(keys(&state), vec!["app:exe:c:/p.exe"]);
        assert!(!state.items[0].needs_attention);

        // A new window of the same app does not inherit the old flash.
        engine.apply(added(win(3, r"C:\p.exe", "P again")));
        assert!(!engine.snapshot().items[0].needs_attention);

        // The same holds when a resync drops the flashing window.
        engine.apply(attention(3, true));
        assert!(engine.snapshot().items[0].needs_attention);
        engine.apply(PlatformEvent::WindowsSnapshot(vec![]).into());
        assert!(!engine.snapshot().items[0].needs_attention);
    }

    #[test]
    fn each_window_carries_its_own_attention_flag() {
        // Two windows of one app: the item needs attention while any
        // of them does, and the snapshot says which one.
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A one")));
        engine.apply(added(win(2, r"C:\a.exe", "A two")));
        let flags = |engine: &DockEngine| -> (bool, Vec<bool>) {
            let item = engine.snapshot().items[0].clone();
            let windows = item.windows.iter().map(|w| w.needs_attention).collect();
            (item.needs_attention, windows)
        };
        assert_eq!(flags(&engine), (false, vec![false, false]));

        assert!(engine.apply(attention(2, true)).changed);
        assert_eq!(flags(&engine), (true, vec![false, true]));

        // The other window flashes too; then the first one stops: the item
        // still needs attention, but the snapshot changed (which window).
        assert!(engine.apply(attention(1, true)).changed);
        assert_eq!(flags(&engine), (true, vec![true, true]));
        let revision = engine.snapshot().revision;
        assert!(engine.apply(attention(1, false)).changed);
        assert_eq!(flags(&engine), (true, vec![false, true]));
        assert_eq!(engine.snapshot().revision, revision + 1);

        // Activating the flashing window clears its own flag.
        engine.apply(foreground(Some(2)));
        assert_eq!(flags(&engine), (false, vec![false, false]));

        // The foreground window is never flagged; the one behind it is.
        assert!(!engine.apply(attention(2, true)).changed);
        assert!(engine.apply(attention(1, true)).changed);
        assert_eq!(flags(&engine), (true, vec![true, false]));
    }

    #[test]
    fn attention_and_removal_of_unknown_windows_are_ignored() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        let before = engine.snapshot();
        assert!(!engine.apply(attention(99, true)).changed);
        assert!(!engine.apply(removed(99)).changed);
        assert!(!engine.apply(attention(1, false)).changed);
        assert_eq!(engine.snapshot(), before);
    }

    #[test]
    fn item_key_is_stable_across_pin_and_unpin() {
        let mut settings = minimal_settings();
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(added(win(2, r"C:\b.exe", "B")));
        let running_key = engine.snapshot().items[1].key.clone();
        assert_eq!(running_key, "app:exe:c:/b.exe");

        settings.pinned = vec![pin(r"C:\b.exe")];
        let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
        assert!(out.changed);
        let state = engine.snapshot();
        assert_eq!(
            keys(&state),
            vec!["app:exe:c:/b.exe", "sep:1", "app:exe:c:/a.exe"]
        );
        assert_eq!(state.items[0].key, running_key);
        assert!(state.items[0].pinned);
        assert_eq!(state.items[0].windows.len(), 1);

        settings.pinned.clear();
        engine.apply(EngineInput::SettingsChanged(settings));
        let state = engine.snapshot();
        // Back in the running group, in its original first-seen slot.
        assert_eq!(keys(&state), vec!["app:exe:c:/a.exe", "app:exe:c:/b.exe"]);
        assert_eq!(state.items[1].key, running_key);
        assert!(!state.items[1].pinned);

        // Unpinning an app with no windows removes its item.
        engine.apply(removed(2));
        assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/a.exe"]);
    }

    #[test]
    fn monitors_and_theme_changes_do_not_bump_the_snapshot() {
        let mut engine = DockEngine::new(Settings::default());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        let before = engine.snapshot();

        let monitors = vec![MonitorInfo {
            id: MonitorId("m1".into()),
            name: "Main".into(),
            bounds: Rect::new(0, 0, 1920, 1080),
            work_area: Rect::new(0, 0, 1920, 1032),
            scale_factor: 1.25,
            is_primary: true,
        }];
        let out = engine.apply(PlatformEvent::MonitorsChanged(monitors.clone()).into());
        assert!(!out.changed);
        assert_eq!(engine.monitors(), monitors.as_slice());

        let theme = ThemeInfo {
            accent: "#FF0000".into(),
            dark: false,
            high_contrast: true,
            transparency: false,
        };
        let out = engine.apply(PlatformEvent::ThemeChanged(theme.clone()).into());
        assert!(!out.changed);
        assert_eq!(engine.theme(), &theme);

        assert_eq!(engine.snapshot(), before);
    }

    #[test]
    fn recycle_bin_changes_bump_only_when_the_icon_flips() {
        let mut engine = DockEngine::new(Settings::default());
        let bin = |count: u64| -> EngineInput {
            PlatformEvent::RecycleBinChanged(RecycleBinState {
                item_count: count,
                size_bytes: count * 100,
            })
            .into()
        };
        let icon = |engine: &DockEngine| {
            engine
                .snapshot()
                .items
                .last()
                .and_then(|i| i.icon_key.clone())
        };
        assert_eq!(icon(&engine), Some(IconKey::shell("recycle-bin-empty")));

        assert!(engine.apply(bin(3)).changed);
        assert_eq!(icon(&engine), Some(IconKey::shell("recycle-bin-full")));
        let r = engine.snapshot().revision;
        assert!(!engine.apply(bin(4)).changed);
        assert_eq!(engine.snapshot().revision, r);
        assert_eq!(engine.recycle_bin().item_count, 4);
        assert!(engine.apply(bin(0)).changed);
        assert_eq!(icon(&engine), Some(IconKey::shell("recycle-bin-empty")));

        // Without the item, the bin state is invisible and never bumps.
        let mut engine = DockEngine::new(minimal_settings());
        assert!(!engine.apply(bin(3)).changed);
        assert!(!engine.apply(bin(0)).changed);
        assert_eq!(engine.snapshot().revision, 0);
    }

    #[test]
    fn settings_changes_bump_only_when_items_change() {
        let mut settings = minimal_settings();
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        let r = engine.snapshot().revision;

        settings.icon_size = 96;
        settings.magnification.max_scale = 2.0;
        settings.appearance.opacity = 0.5;
        let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
        assert!(!out.changed);
        assert_eq!(engine.snapshot().revision, r);
        assert_eq!(engine.settings().icon_size, 96);

        settings.items.launcher = true;
        let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
        assert!(out.changed);
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["fixed:launcher", "sep:1", "app:exe:c:/a.exe"]
        );

        settings.items.separators = false;
        let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
        assert!(out.changed);
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["fixed:launcher", "app:exe:c:/a.exe"]
        );

        // Re-applying identical settings is a no-op.
        assert!(!engine.apply(EngineInput::SettingsChanged(settings)).changed);
    }

    #[test]
    fn identity_for_an_absent_app_is_kept_without_bumping() {
        let mut engine = DockEngine::new(minimal_settings());
        let a = exe_id(r"C:\a.exe");
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(removed(1));
        let r = engine.snapshot().revision;

        // The resolution arrives after the window closed (service latency).
        let out = engine.apply(EngineInput::IdentityResolved {
            app_id: a.clone(),
            identity: identity(&a, "App A", "k"),
        });
        assert!(!out.changed);
        assert_eq!(engine.snapshot().revision, r);

        // When it comes back it is named right away and not requested again.
        let out = engine.apply(added(win(2, r"C:\a.exe", "A")));
        assert!(out.changed);
        assert!(out.unresolved_apps.is_empty());
        assert_eq!(engine.snapshot().items[0].title, "App A");

        // Resolving to the same identity again is a no-op.
        let out = engine.apply(EngineInput::IdentityResolved {
            app_id: a.clone(),
            identity: identity(&a, "App A", "k"),
        });
        assert!(!out.changed);
    }

    #[test]
    fn identity_unavailable_forgets_the_request_without_a_fallback() {
        let mut engine = DockEngine::new(minimal_settings());
        let a = exe_id(r"C:\a.exe");
        let out = engine.apply(added(win(1, r"C:\a.exe", "A")));
        assert_eq!(out.unresolved_apps, vec![a.clone()]);
        engine.apply(removed(1));
        let r = engine.snapshot().revision;

        // The resolver found nothing to ask (no window, no pin): nothing is
        // stored, nothing is shown, nothing is re-requested yet.
        let out = engine.apply(EngineInput::IdentityUnavailable { app_id: a.clone() });
        assert!(!out.changed);
        assert!(out.unresolved_apps.is_empty());
        assert_eq!(engine.snapshot().revision, r);
        assert_eq!(engine.identity(&a), None);
        assert!(engine.unresolved_apps().is_empty());

        // The app comes back: its identity is requested again, and the item
        // shows the window title until it is resolved (no fallback name).
        let out = engine.apply(added(win(2, r"C:\a.exe", "A again")));
        assert!(out.changed);
        assert_eq!(out.unresolved_apps, vec![a.clone()]);
        assert_eq!(engine.snapshot().items[0].title, "A again");
        assert_eq!(engine.unresolved_apps(), vec![a.clone()]);

        // Unavailable while the app is present (it re-appeared before the
        // message arrived): the same input re-requests it immediately.
        let out = engine.apply(EngineInput::IdentityUnavailable { app_id: a.clone() });
        assert!(!out.changed);
        assert_eq!(out.unresolved_apps, vec![a.clone()]);

        // An unknown id is ignored.
        let out = engine.apply(EngineInput::IdentityUnavailable {
            app_id: exe_id(r"C:\none.exe"),
        });
        assert!(!out.changed);
        assert!(out.unresolved_apps.is_empty());
    }

    #[test]
    fn window_updates_without_visible_effect_do_not_bump() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        let r = engine.snapshot().revision;

        let mut same = win(1, r"C:\a.exe", "A");
        same.process_id = 4242;
        same.class_name = "Other".into();
        assert!(!engine.apply(updated(same)).changed);
        assert_eq!(engine.snapshot().revision, r);
        assert_eq!(engine.window(WindowId(1)).map(|w| w.process_id), Some(4242));

        let mut renamed = win(1, r"C:\a.exe", "A - edited");
        renamed.is_minimized = true;
        assert!(engine.apply(updated(renamed)).changed);
        let item = &engine.snapshot().items[0];
        assert_eq!(item.windows[0].title, "A - edited");
        assert!(item.windows[0].is_minimized);
    }

    #[test]
    fn foreground_to_an_untracked_window_clears_the_active_marker() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(foreground(Some(1)));
        assert!(engine.snapshot().items[0].is_active);

        // A tool window or the desktop took focus: not something we track.
        let out = engine.apply(foreground(Some(999)));
        assert!(out.changed);
        assert_eq!(engine.foreground(), None);
        assert!(!engine.snapshot().items[0].is_active);
        assert!(!engine.apply(foreground(None)).changed);
    }

    #[test]
    fn duplicate_window_added_is_treated_as_an_update() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(added(win(2, r"C:\b.exe", "B")));
        engine.apply(attention(1, true));

        let out = engine.apply(added(win(1, r"C:\a.exe", "A renamed")));
        assert!(out.changed);
        let state = engine.snapshot();
        // Still one window, still first, attention untouched.
        assert_eq!(keys(&state), vec!["app:exe:c:/a.exe", "app:exe:c:/b.exe"]);
        assert_eq!(state.items[0].windows.len(), 1);
        assert_eq!(state.items[0].windows[0].title, "A renamed");
        assert!(state.items[0].needs_attention);
    }

    #[test]
    fn windows_of_lists_shown_windows_in_first_seen_order() {
        let mut engine = DockEngine::new(minimal_settings());
        let a = exe_id(r"C:\a.exe");
        engine.apply(added(win(3, r"C:\a.exe", "third id, first seen")));
        engine.apply(added(win(1, r"C:\b.exe", "other app")));
        let mut minimized = win(2, r"C:\a.exe", "minimised");
        minimized.is_minimized = true;
        engine.apply(added(minimized));
        engine.apply(added(cloaked(win(4, r"C:\a.exe", "cloaked"))));
        let mut invisible = win(5, r"C:\a.exe", "invisible");
        invisible.is_visible = false;
        engine.apply(added(invisible));

        assert_eq!(engine.windows_of(&a), vec![WindowId(3), WindowId(2)]);
        assert_eq!(engine.windows_of(&exe_id(r"C:\b.exe")), vec![WindowId(1)]);
        assert!(engine.windows_of(&exe_id(r"C:\none.exe")).is_empty());
        let item = &engine.snapshot().items[0];
        let listed: Vec<WindowId> = item.windows.iter().map(|w| w.id).collect();
        assert_eq!(listed, engine.windows_of(&a));
    }

    #[test]
    fn primary_action_cycles_through_shown_windows_only() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A1")));
        engine.apply(added(cloaked(win(2, r"C:\a.exe", "A2 on another desktop"))));
        engine.apply(added(win(3, r"C:\a.exe", "A3")));
        engine.apply(foreground(Some(1)));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::ActivateWindow(WindowId(3))
        );
        engine.apply(foreground(Some(3)));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::ActivateWindow(WindowId(1))
        );

        // Only the hidden window left: it behaves like a closed app.
        engine.apply(removed(1));
        engine.apply(removed(3));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::None
        );
    }

    // ----- clicks and menus act on the windows the dock shows -------------------

    const NOTEPAD: &str = r"C:\Windows\notepad.exe";
    const NOTEPAD_KEY: &str = "app:exe:c:/windows/notepad.exe";
    /// The second monitor's dock window, as the app layer labels it.
    const SECOND_DOCK: &str = "dock-DISPLAY3";

    fn on_monitor(mut raw: RawWindow, monitor: &str) -> RawWindow {
        raw.monitor = Some(MonitorId(monitor.into()));
        raw
    }

    fn ids(windows: &[WindowInfo]) -> Vec<u64> {
        windows.iter().map(|w| w.id.0).collect()
    }

    /// A dock on each of two monitors (`monitors = all`), with
    /// `showOnlyCurrentMonitor` as given; Notepad (pinned or not) has
    /// window 1 ("A") on the primary, seen first, and window 2 ("B") on the
    /// second monitor.
    fn two_docks(pinned: bool, show_only_current_monitor: bool) -> DockEngine {
        let mut settings = minimal_settings();
        settings.monitors = MonitorMode::All;
        settings.behavior.show_only_current_monitor = show_only_current_monitor;
        if pinned {
            settings.pinned = vec![pin(NOTEPAD)];
        }
        let mut engine = DockEngine::new(settings);
        engine.apply(monitors(vec![
            monitor(PRIMARY, true, 0),
            monitor(SECOND, false, 2560),
        ]));
        engine.apply(panel_of(DOCK, Some(PRIMARY), None));
        engine.apply(panel_of(SECOND_DOCK, Some(SECOND), None));
        engine.apply(added(on_monitor(win(1, NOTEPAD, "A"), PRIMARY)));
        engine.apply(added(on_monitor(win(2, NOTEPAD, "B"), SECOND)));
        engine
    }

    #[test]
    fn a_click_acts_on_the_windows_on_the_docks_own_monitor() {
        let mut engine = two_docks(false, true);
        let notepad = exe_id(NOTEPAD);
        // A cloaked window on the second monitor is hidden there as anywhere.
        engine.apply(added(on_monitor(cloaked(win(9, NOTEPAD, "away")), SECOND)));
        assert_eq!(
            engine.windows_on_dock(&notepad, SECOND_DOCK),
            vec![WindowId(2)]
        );
        assert_eq!(engine.windows_on_dock(&notepad, DOCK), vec![WindowId(1)]);

        // Nothing active: each dock activates its own window, not the
        // app's first-seen one.
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
            ItemAction::ActivateWindow(WindowId(2))
        );
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, DOCK),
            ItemAction::ActivateWindow(WindowId(1))
        );

        // B active: its dock minimises it instead of cycling to A; the
        // primary's dock, which shows only A, activates A.
        engine.apply(foreground(Some(2)));
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
            ItemAction::MinimizeWindow(WindowId(2))
        );
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, DOCK),
            ItemAction::ActivateWindow(WindowId(1))
        );

        // Without clickActiveMinimizes the active window is activated again.
        let mut settings = engine.settings().clone();
        settings.behavior.click_active_minimizes = false;
        engine.apply(EngineInput::SettingsChanged(settings.clone()));
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
            ItemAction::ActivateWindow(WindowId(2))
        );
        settings.behavior.click_active_minimizes = true;
        engine.apply(EngineInput::SettingsChanged(settings));

        // Two windows on one monitor cycle among themselves, skipping A.
        engine.apply(added(on_monitor(win(3, NOTEPAD, "C"), SECOND)));
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
            ItemAction::ActivateWindow(WindowId(3))
        );
        engine.apply(foreground(Some(3)));
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
            ItemAction::ActivateWindow(WindowId(2))
        );

        // A window moved to the other monitor follows at once.
        engine.apply(updated(on_monitor(win(1, NOTEPAD, "A"), SECOND)));
        assert_eq!(
            engine.windows_on_dock(&notepad, SECOND_DOCK),
            vec![WindowId(1), WindowId(2), WindowId(3)]
        );
        assert!(engine.windows_on_dock(&notepad, DOCK).is_empty());

        // Fixed items are the same on every dock.
        assert_eq!(
            engine.primary_action("fixed:launcher", SECOND_DOCK),
            ItemAction::Shell(ShellAction::OpenStartMenu)
        );
    }

    #[test]
    fn a_docks_menu_item_lists_only_the_windows_on_its_monitor() {
        let mut engine = two_docks(false, true);
        engine.apply(foreground(Some(1)));
        engine.apply(attention(2, true));

        // The second dock: B only; A is active, B asks for attention.
        let item = engine.dock_item(NOTEPAD_KEY, SECOND_DOCK).unwrap();
        assert_eq!(ids(&item.windows), vec![2]);
        assert!(!item.is_active);
        assert!(item.needs_attention);
        assert_eq!(item.title, "A");
        // The primary's: A only, active, and no attention: only B flashed.
        let item = engine.dock_item(NOTEPAD_KEY, DOCK).unwrap();
        assert_eq!(ids(&item.windows), vec![1]);
        assert!(item.is_active);
        assert!(!item.needs_attention);

        // A dock that lists none of the windows keeps the item, without
        // windows, active state or attention.
        engine.apply(updated(on_monitor(win(1, NOTEPAD, "A"), SECOND)));
        let item = engine.dock_item(NOTEPAD_KEY, DOCK).unwrap();
        assert!(item.windows.is_empty());
        assert!(!item.is_active);
        assert!(!item.needs_attention);

        // Other items are the snapshot's; unknown keys have none.
        let mut settings = engine.settings().clone();
        settings.items.launcher = true;
        engine.apply(EngineInput::SettingsChanged(settings));
        let snapshot = engine.snapshot();
        assert_eq!(
            engine.dock_item("fixed:launcher", SECOND_DOCK).as_ref(),
            snapshot.items.iter().find(|i| i.key == "fixed:launcher")
        );
        assert_eq!(engine.dock_item("app:exe:c:/nope.exe", SECOND_DOCK), None);
    }

    #[test]
    fn a_pinned_app_with_no_window_on_the_docks_monitor_is_launched_there() {
        let mut engine = two_docks(true, true);
        engine.apply(removed(1));
        // Only B is left, on the second monitor: the primary's dock shows no
        // window for Notepad and launches it; the second dock activates B.
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, DOCK),
            ItemAction::Launch(exe_id(NOTEPAD))
        );
        assert_eq!(
            engine.primary_action(NOTEPAD_KEY, SECOND_DOCK),
            ItemAction::ActivateWindow(WindowId(2))
        );
        let item = engine.dock_item(NOTEPAD_KEY, DOCK).unwrap();
        assert!(item.pinned);
        assert!(item.windows.is_empty());

        // Unpinned and without a launch target: nothing, as for a closed app.
        let mut engine = two_docks(false, true);
        engine.apply(removed(1));
        assert_eq!(engine.primary_action(NOTEPAD_KEY, DOCK), ItemAction::None);
    }

    #[test]
    fn every_window_counts_with_one_dock_the_setting_off_or_an_unknown_monitor() {
        let notepad = exe_id(NOTEPAD);
        let every = vec![WindowId(1), WindowId(2)];
        let assert_unnarrowed = |engine: &DockEngine, dock: &str| {
            assert_eq!(engine.windows_on_dock(&notepad, dock), every, "{dock}");
            assert_eq!(
                engine.windows_on_dock(&notepad, dock),
                engine.windows_of(&notepad),
                "{dock}"
            );
            assert_eq!(
                engine.primary_action(NOTEPAD_KEY, dock),
                ItemAction::ActivateWindow(WindowId(1)),
                "{dock}"
            );
            let item = engine.dock_item(NOTEPAD_KEY, dock).unwrap();
            assert_eq!(ids(&item.windows), vec![1, 2], "{dock}");
        };

        // The setting off: every dock lists everything.
        let mut engine = two_docks(false, false);
        for dock in [DOCK, SECOND_DOCK, "dock-DISPLAY9"] {
            assert_unnarrowed(&engine, dock);
        }

        // On, but with one dock (`monitors = primary`): today's behaviour,
        // whatever the app layer last registered.
        let mut settings = engine.settings().clone();
        settings.behavior.show_only_current_monitor = true;
        settings.monitors = MonitorMode::Primary;
        engine.apply(EngineInput::SettingsChanged(settings.clone()));
        for dock in [DOCK, SECOND_DOCK] {
            assert_unnarrowed(&engine, dock);
        }

        // `monitors = all` again: narrowed, read at click time.
        settings.monitors = MonitorMode::All;
        engine.apply(EngineInput::SettingsChanged(settings));
        assert_eq!(
            engine.windows_on_dock(&notepad, SECOND_DOCK),
            vec![WindowId(2)]
        );

        // A label the app layer never registered, and a dock window it
        // could not place: every window.
        assert_unnarrowed(&engine, "dock-DISPLAY9");
        engine.apply(panel_of(
            SECOND_DOCK,
            None,
            Some(Rect::new(3000, 1000, 560, 72)),
        ));
        assert_eq!(engine.dock_monitor(SECOND_DOCK), None);
        assert_unnarrowed(&engine, SECOND_DOCK);

        // A window whose monitor the platform could not tell is on no
        // dock's monitor, as on the page.
        engine.apply(updated(win(1, NOTEPAD, "A")));
        assert!(engine.windows_on_dock(&notepad, DOCK).is_empty());
        assert_eq!(engine.windows_of(&notepad), every);
    }
}
