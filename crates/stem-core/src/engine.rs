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
//! - [`EngineOutcome::changed`] is defined by comparing snapshots, not by what
//!   kind of input arrived, so inputs that only touch facts outside the
//!   snapshot (monitors, theme, launch targets, identities of apps that are
//!   not shown) never bump the revision;
//! - `foreground` is the OS truth and is only ever an id the engine tracks. It
//!   may point at a window no item shows (cloaked, invisible); nothing is
//!   marked active then, and the item lights up as soon as the window is shown
//!   again without needing another `ForegroundChanged`.
//! - `fullscreen` is the platform's verdict, stored as is: the engine
//!   never derives it from window rects (`RawWindow::rect` is kept but not
//!   rendered, so a rect change alone never bumps the revision).

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
    /// The renderable snapshot (items or foreground) differs from the one
    /// before this input; emit a new `dock://state`. Inputs that only change
    /// [`DockEngine::monitors`], [`DockEngine::theme`], settings that do not
    /// alter items, or identities of apps that are not shown leave this
    /// `false` and do not advance the revision.
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
    /// The platform's fullscreen verdict; see `DockState::fullscreen`.
    fullscreen: bool,
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

    /// The OS foreground window, if the engine tracks it. It may be a window
    /// that no item shows (cloaked or invisible).
    pub fn foreground(&self) -> Option<WindowId> {
        self.foreground
    }

    /// The platform's latest fullscreen verdict.
    pub fn fullscreen(&self) -> bool {
        self.fullscreen
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
        }
        self.rebuild_running_order();
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
            }
            PlatformEvent::WindowAdded(raw) | PlatformEvent::WindowUpdated(raw) => self.upsert(raw),
            PlatformEvent::WindowRemoved(id) => {
                self.windows.remove(&id);
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
            // Part of the snapshot, so `apply` bumps the revision only when
            // the value differs from the stored one.
            PlatformEvent::FullscreenChanged(fullscreen) => self.fullscreen = fullscreen,
        }
    }

    fn upsert(&mut self, raw: RawWindow) {
        let id = raw.id;
        match self.windows.get_mut(&id) {
            Some(t) => t.raw = raw,
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
            fullscreen: self.fullscreen,
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
    /// - an app with no shown windows is launched (if it can be);
    /// - an app whose window is active is minimised (if configured), otherwise
    ///   activated;
    /// - an app with several windows cycles: the window after the active one,
    ///   or the first one. Only shown windows take part.
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
        let windows = self.windows_of(&app_id);
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
            monitor: None,
            rect: None,
            icon_key: None,
        }
    }

    fn cloaked(mut raw: RawWindow) -> RawWindow {
        raw.is_cloaked = true;
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

    fn fullscreen(active: bool) -> EngineInput {
        PlatformEvent::FullscreenChanged(active).into()
    }

    #[test]
    fn fullscreen_is_stored_and_bumps_only_on_a_change() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        let r = engine.snapshot().revision;
        assert!(!engine.snapshot().fullscreen);
        assert!(!engine.fullscreen());

        // The platform's first verdict is usually `false`: nothing to emit.
        assert!(!engine.apply(fullscreen(false)).changed);
        assert_eq!(engine.snapshot().revision, r);

        let out = engine.apply(fullscreen(true));
        assert!(out.changed);
        assert!(out.unresolved_apps.is_empty());
        let state = engine.snapshot();
        assert!(state.fullscreen);
        assert!(engine.fullscreen());
        assert_eq!(state.revision, r + 1);
        // Items and foreground are untouched by it.
        assert_eq!(keys(&state), vec!["app:exe:c:/a.exe"]);

        // Repeats change nothing; the end of fullscreen is one more bump.
        assert!(!engine.apply(fullscreen(true)).changed);
        assert_eq!(engine.snapshot().revision, r + 1);
        assert!(engine.apply(fullscreen(false)).changed);
        assert!(!engine.snapshot().fullscreen);
        assert_eq!(engine.snapshot().revision, r + 2);
    }

    #[test]
    fn fullscreen_survives_a_resync_and_is_independent_of_the_foreground() {
        let mut engine = DockEngine::new(minimal_settings());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(fullscreen(true));
        engine.apply(foreground(Some(1)));
        engine.apply(PlatformEvent::WindowsSnapshot(vec![win(1, r"C:\a.exe", "A")]).into());
        engine.apply(foreground(None));
        engine.apply(removed(1));
        // Only the platform says when it is over.
        assert!(engine.snapshot().fullscreen);
    }

    #[test]
    fn a_rect_change_alone_is_stored_without_bumping_the_revision() {
        let mut engine = DockEngine::new(minimal_settings());
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
        assert!(!engine.apply(updated(raw)).changed);
        assert_eq!(engine.window(WindowId(1)).and_then(|w| w.rect), None);
        assert_eq!(engine.snapshot().revision, r);
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
            engine.primary_action("app:exe:c:/p.exe"),
            ItemAction::Launch(p)
        );

        // One window, not active -> activate.
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe"),
            ItemAction::ActivateWindow(WindowId(1))
        );

        // One window, active -> minimise.
        engine.apply(foreground(Some(1)));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe"),
            ItemAction::MinimizeWindow(WindowId(1))
        );

        // Two windows, first active -> cycle to the second.
        engine.apply(added(win(2, r"C:\a.exe", "A2")));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe"),
            ItemAction::ActivateWindow(WindowId(2))
        );

        // Unknown running app with no launch info -> nothing after it closes.
        engine.apply(removed(1));
        engine.apply(removed(2));
        assert_eq!(engine.primary_action("app:exe:c:/a.exe"), ItemAction::None);

        // Fixed items.
        assert_eq!(
            engine.primary_action("fixed:launcher"),
            ItemAction::Shell(ShellAction::OpenStartMenu)
        );
        assert_eq!(engine.primary_action("nonsense"), ItemAction::None);
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
            engine.primary_action("app:exe:c:/a.exe"),
            ItemAction::ActivateWindow(WindowId(1))
        );

        // Several windows still cycle, as with the setting on.
        engine.apply(added(win(2, r"C:\a.exe", "A2")));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe"),
            ItemAction::ActivateWindow(WindowId(2))
        );

        // The setting is read at click time: turning it on minimises again.
        engine.apply(removed(2));
        settings.behavior.click_active_minimizes = true;
        engine.apply(EngineInput::SettingsChanged(settings));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe"),
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
            engine.primary_action("app:exe:c:/a.exe"),
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
        assert_eq!(engine.primary_action("app:exe:c:/a.exe"), ItemAction::None);

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
            engine.primary_action("app:exe:c:/a.exe"),
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
            engine.primary_action("app:exe:c:/a.exe"),
            ItemAction::ActivateWindow(WindowId(3))
        );
        engine.apply(foreground(Some(3)));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe"),
            ItemAction::ActivateWindow(WindowId(1))
        );

        // Only the hidden window left: it behaves like a closed app.
        engine.apply(removed(1));
        engine.apply(removed(3));
        assert_eq!(engine.primary_action("app:exe:c:/a.exe"), ItemAction::None);
    }
}
