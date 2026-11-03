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

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::events::{EngineInput, PlatformEvent};
use crate::model::{
    AppId, AppIdentity, DockItem, DockItemKind, DockState, IconKey, MonitorInfo, RawWindow,
    RecycleBinState, ShellAction, ThemeInfo, WindowId, WindowInfo,
};
use crate::settings::Settings;

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
    /// The snapshot changed; emit a new `dock://state`.
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

#[derive(Debug)]
pub struct DockEngine {
    settings: Settings,
    windows: BTreeMap<WindowId, Tracked>,
    identities: HashMap<AppId, AppIdentity>,
    identity_requested: HashSet<AppId>,
    /// First-seen order of apps that currently have windows.
    running_order: Vec<AppId>,
    foreground: Option<WindowId>,
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
            monitors: Vec::new(),
            theme: ThemeInfo::default(),
            recycle_bin: RecycleBinState::default(),
            next_seq: 1,
            revision: 0,
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

    pub fn foreground(&self) -> Option<WindowId> {
        self.foreground
    }

    pub fn window(&self, id: WindowId) -> Option<&RawWindow> {
        self.windows.get(&id).map(|t| &t.raw)
    }

    pub fn identity(&self, id: &AppId) -> Option<&AppIdentity> {
        self.identities.get(id)
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
    pub fn apply(&mut self, input: EngineInput) -> EngineOutcome {
        let changed = match input {
            EngineInput::Platform(ev) => self.apply_platform(ev),
            EngineInput::SettingsChanged(settings) => {
                let changed = settings != self.settings;
                self.settings = settings;
                changed
            }
            EngineInput::IdentityResolved { app_id, identity } => {
                self.identity_requested.remove(&app_id);
                let changed = self.identities.get(&app_id) != Some(&identity);
                self.identities.insert(app_id, identity);
                changed
            }
        };
        self.rebuild_running_order();
        let unresolved_apps = self.take_new_unresolved();
        if changed {
            self.revision += 1;
        }
        EngineOutcome {
            changed,
            unresolved_apps,
        }
    }

    fn apply_platform(&mut self, ev: PlatformEvent) -> bool {
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
                let changed = next != self.windows;
                self.windows = next;
                changed
            }
            PlatformEvent::WindowAdded(raw) | PlatformEvent::WindowUpdated(raw) => self.upsert(raw),
            PlatformEvent::WindowRemoved(id) => {
                let removed = self.windows.remove(&id).is_some();
                if self.foreground == Some(id) {
                    self.foreground = None;
                }
                removed
            }
            PlatformEvent::ForegroundChanged(id) => {
                let id = id.filter(|id| self.windows.contains_key(id));
                let mut changed = self.foreground != id;
                self.foreground = id;
                if let Some(t) = id.and_then(|id| self.windows.get_mut(&id)) {
                    // Activating a window clears its attention request.
                    changed |= std::mem::replace(&mut t.attention, false);
                }
                changed
            }
            PlatformEvent::WindowAttention { id, flashing } => match self.windows.get_mut(&id) {
                Some(t) => std::mem::replace(&mut t.attention, flashing) != flashing,
                None => false,
            },
            PlatformEvent::MonitorsChanged(monitors) => {
                let changed = monitors != self.monitors;
                self.monitors = monitors;
                changed
            }
            PlatformEvent::ThemeChanged(theme) => {
                let changed = theme != self.theme;
                self.theme = theme;
                changed
            }
            PlatformEvent::RecycleBinChanged(state) => {
                let changed = state != self.recycle_bin;
                self.recycle_bin = state;
                changed
            }
        }
    }

    fn upsert(&mut self, raw: RawWindow) -> bool {
        let id = raw.id;
        match self.windows.get_mut(&id) {
            Some(t) => {
                if t.raw == raw {
                    false
                } else {
                    t.raw = raw;
                    true
                }
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
                true
            }
        }
    }

    fn alloc_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    fn is_shown(&self, raw: &RawWindow) -> bool {
        raw.is_visible && !(raw.is_cloaked && self.settings.behavior.show_only_current_desktop)
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
            needs_attention: windows.iter().any(|t| t.attention),
            windows: infos,
            badge: None,
        }
    }

    /// Decide what a primary click on `item_key` should do.
    ///
    /// - fixed items map to their shell action;
    /// - an app with no windows is launched (if it can be);
    /// - an app whose window is active is minimised (if configured), otherwise
    ///   activated;
    /// - an app with several windows cycles: the window after the active one,
    ///   or the first one.
    pub fn primary_action(&self, item_key: &str) -> ItemAction {
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
        let windows: Vec<WindowId> = self
            .shown_windows()
            .into_iter()
            .filter(|t| app_id_for(&t.raw) == app_id)
            .map(|t| t.raw.id)
            .collect();
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
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::model::LaunchTarget;
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
            monitor: None,
            icon_key: None,
        }
    }

    fn pin(exe: &str) -> PinnedApp {
        PinnedApp {
            id: AppId::for_exe(std::path::Path::new(exe)),
            label: None,
            launch: LaunchTarget::Exe {
                path: PathBuf::from(exe),
                args: vec![],
                working_dir: None,
            },
        }
    }

    fn keys(state: &DockState) -> Vec<&str> {
        state.items.iter().map(|i| i.key.as_str()).collect()
    }

    fn minimal_settings() -> Settings {
        let mut s = Settings::default();
        s.items.launcher = false;
        s.items.recycle_bin = false;
        s
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
        let out = engine.apply(PlatformEvent::WindowAdded(win(1, r"C:\b.exe", "B1")).into());
        assert!(out.changed);
        assert_eq!(
            out.unresolved_apps,
            vec![AppId::for_exe(std::path::Path::new(r"C:\b.exe"))]
        );
        engine.apply(PlatformEvent::WindowAdded(win(2, r"C:\a.exe", "A1")).into());
        engine.apply(PlatformEvent::WindowAdded(win(3, r"C:\b.exe", "B2")).into());

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
        engine.apply(PlatformEvent::WindowAdded(win(1, r"C:\b.exe", "B")).into());
        engine.apply(PlatformEvent::WindowAdded(win(2, r"C:\a.exe", "A")).into());

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
        engine.apply(PlatformEvent::WindowAdded(win(1, r"C:\a.exe", "A")).into());
        engine.apply(
            PlatformEvent::WindowAttention {
                id: WindowId(1),
                flashing: true,
            }
            .into(),
        );
        assert!(engine.snapshot().items[0].needs_attention);
        let out = engine.apply(PlatformEvent::ForegroundChanged(Some(WindowId(1))).into());
        assert!(out.changed);
        let item = &engine.snapshot().items[0];
        assert!(item.is_active);
        assert!(!item.needs_attention);
    }

    #[test]
    fn cloaked_windows_are_hidden_by_default() {
        let mut engine = DockEngine::new(minimal_settings());
        let mut w = win(1, r"C:\a.exe", "A");
        w.is_cloaked = true;
        engine.apply(PlatformEvent::WindowAdded(w).into());
        assert!(engine.snapshot().items.is_empty());
    }

    #[test]
    fn snapshot_resync_preserves_order_and_drops_vanished() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(PlatformEvent::WindowAdded(win(1, r"C:\a.exe", "A")).into());
        engine.apply(PlatformEvent::WindowAdded(win(2, r"C:\b.exe", "B")).into());
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
        let p = AppId::for_exe(std::path::Path::new(r"C:\p.exe"));

        // Pinned, not running -> launch.
        assert_eq!(
            engine.primary_action("app:exe:c:/p.exe"),
            ItemAction::Launch(p)
        );

        // One window, not active -> activate.
        engine.apply(PlatformEvent::WindowAdded(win(1, r"C:\a.exe", "A")).into());
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe"),
            ItemAction::ActivateWindow(WindowId(1))
        );

        // One window, active -> minimise.
        engine.apply(PlatformEvent::ForegroundChanged(Some(WindowId(1))).into());
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe"),
            ItemAction::MinimizeWindow(WindowId(1))
        );

        // Two windows, first active -> cycle to the second.
        engine.apply(PlatformEvent::WindowAdded(win(2, r"C:\a.exe", "A2")).into());
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe"),
            ItemAction::ActivateWindow(WindowId(2))
        );

        // Unknown running app with no launch info -> nothing after it closes.
        engine.apply(PlatformEvent::WindowRemoved(WindowId(1)).into());
        engine.apply(PlatformEvent::WindowRemoved(WindowId(2)).into());
        assert_eq!(engine.primary_action("app:exe:c:/a.exe"), ItemAction::None);

        // Fixed items.
        assert_eq!(
            engine.primary_action("fixed:launcher"),
            ItemAction::Shell(ShellAction::OpenStartMenu)
        );
        assert_eq!(engine.primary_action("nonsense"), ItemAction::None);
    }

    #[test]
    fn identity_is_requested_once_until_resolved() {
        let mut engine = DockEngine::new(minimal_settings());
        let a = AppId::for_exe(std::path::Path::new(r"C:\a.exe"));
        let out = engine.apply(PlatformEvent::WindowAdded(win(1, r"C:\a.exe", "A")).into());
        assert_eq!(out.unresolved_apps, vec![a.clone()]);
        let out = engine.apply(PlatformEvent::WindowAdded(win(2, r"C:\a.exe", "A2")).into());
        assert!(out.unresolved_apps.is_empty());
        assert_eq!(engine.unresolved_apps(), vec![a.clone()]);

        let identity = AppIdentity {
            id: a.clone(),
            display_name: "App A".into(),
            icon_key: Some(IconKey("k".into())),
            launch: None,
        };
        let out = engine.apply(EngineInput::IdentityResolved {
            app_id: a.clone(),
            identity,
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
        engine.apply(PlatformEvent::WindowAdded(win(1, r"C:\a.exe", "A")).into());
        let r1 = engine.snapshot().revision;
        assert!(r1 > r0);
        let out = engine.apply(PlatformEvent::WindowUpdated(win(1, r"C:\a.exe", "A")).into());
        assert!(!out.changed);
        assert_eq!(engine.snapshot().revision, r1);
    }
}
