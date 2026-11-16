//! The dock engine: a pure, synchronous state machine.
//!
//! Feed it [`EngineInput`]s, ask it for a [`DockState`] snapshot, and ask it
//! what a click on an item should do. It never touches the OS, which is what
//! makes the project's behaviour testable on any host.
//!
//! Ordering rules for the snapshot (left to right):
//! 1. fixed left items (launcher, task view, show desktop), then a separator;
//! 2. pinned apps in the user's order, each showing its running windows;
//! 3. a separator, then running apps that are not pinned, in first-seen order,
//!    except the ones `Settings::hidden_apps` lists;
//! 4. a separator, then the fixed trailing items (quick settings,
//!    notification center, volume, battery, clock, the Recycle Bin last).
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
//!   of apps that are not shown) never bump the revision. Nor does a reading
//!   no shown item uses: `power` counts only while the battery item
//!   is shown, or when it brings or takes that item, `volume` only while the
//!   volume item is (`renders_differently`).
//!   The monitor list
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
//! - `power` is the platform's last reading, stored as is and part
//!   of the snapshot whatever the settings say; it also decides whether the
//!   battery item exists (only with a system battery) and its title. A new
//!   reading changes the snapshot only while the battery item is shown, or
//!   when it brings or takes that item (`has_battery` flipping with
//!   `items.battery` on);
//! - `volume` is the platform's last reading of the default output
//!   device's volume, stored as is (`None`: no device) and part of the
//!   snapshot whatever the settings say; it gives the volume item its
//!   title, and a new reading changes the snapshot only while that item is
//!   shown. A wheel notch over that item is a volume step
//!   ([`ScrollEffect::StepVolume`], [`volume_step`]) whatever
//!   `behavior.scrollAction` says;
//! - `clock_format` is the platform's last reading of the user's
//!   regional format, stored as is and part of the snapshot whatever the
//!   settings say; the engine draws nothing from it (the page formats the
//!   clock with it), so a different reading bumps the revision and nothing
//!   else;
//! - a hidden app (`Settings::hidden_apps`) only loses its item: its
//!   windows are tracked as before, so the foreground, fullscreen and
//!   overlap facts and its identity are unchanged, and a click on its key
//!   does nothing. A pinned app is never hidden, whatever the list says;
//! - `overlapped` is the one thing the engine derives from window
//!   rects: whether a window of another process covers a dock's resting
//!   panel ([`crate::overlap`]), per panel. It is recomputed only after an
//!   input that can change it (a window's frame, visibility, minimised or
//!   cloaked state, a window coming or going, a resync, a panel itself),
//!   in O(windows × panels), and it is part of the snapshot, so a
//!   `WindowUpdated` that only moves a window bumps the revision exactly
//!   when a flag flips.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::events::{EngineInput, PlatformEvent};
use crate::model::{
    AppId, AppIdentity, ClockFormat, DockFacts, DockItem, DockItemKind, DockState, IconKey,
    MonitorId, MonitorInfo, PowerStatus, RawWindow, Rect, RecycleBinState, ScrollDirection,
    ShellAction, ThemeInfo, VolumeStatus, WindowId, WindowInfo,
};
use crate::overlap;
use crate::settings::{MonitorMode, ScrollAction, Settings};

/// How long a dock window remembers the window its last admitted wheel
/// notch tried to activate ([`DockEngine::note_scroll_activation`];
/// the foreground changing ends it sooner). 2 s: far longer than a late
/// foreground report (tens of milliseconds) or a pause within one spin of
/// the wheel, and short enough that a notch turned later steps from the
/// window in front again, so a window Windows refused is tried once more.
pub const SCROLL_MEMORY: Duration = Duration::from_secs(2);

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
    /// advance the revision. So does a new power reading while the battery
    /// item is not shown, unless it brings that item (`has_battery` turning
    /// on with `items.battery` on), and a new volume reading while the volume
    /// item is not: the snapshot carries them, but no shown item
    /// draws them. (The volume item is shown whenever its setting is on, so
    /// no reading brings or takes it.)
    /// So does a change of [`DockEngine::monitors`],
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

/// The effect of one wheel notch over a dock item
/// (`docs/ipc/scrolling-over-an-item.md`): what
/// [`DockEngine::scroll_action`] decides. How often it may happen is the
/// caller's business (the service's 150 ms gate per dock window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollEffect {
    /// Bring this window to the front (`WindowAction::Activate`, which also
    /// restores a minimised one).
    Activate(WindowId),
    /// Step the default output device's volume by one notch: the
    /// item is the volume item, which the wheel drives whatever
    /// `behavior.scrollAction` says. The caller reads the volume as it is
    /// now and sets what [`volume_step`] makes of it; the 150 ms gate does
    /// not apply, every notch counts.
    StepVolume(ScrollDirection),
    /// Nothing to do: `behavior.scrollAction` is `none`, the item is not an
    /// app (a fixed item other than the volume, a separator), or the dock
    /// lists none of its windows.
    Nothing,
    /// The key names no item of the snapshot: a key the page made up, an
    /// item that has gone, or a hidden app's. The caller rejects it
    /// as `primary_click` rejects such a key.
    UnknownItem,
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
    /// The window each dock window's last admitted wheel notch asked the
    /// platform to activate, and when, by label (see
    /// [`Self::note_scroll_activation`]). Emptied whenever `foreground`
    /// changes; a label also leaves with its panel; an entry older than
    /// [`SCROLL_MEMORY`] counts for nothing.
    scroll_tried: BTreeMap<String, (WindowId, Instant)>,
    /// Our own process id, whose windows never count as overlapping. `None`
    /// when unknown (every window counts then).
    own_process_id: Option<u32>,
    /// Some panel's `overlapped` may be stale: recompute them all in
    /// [`Self::apply`].
    overlap_dirty: bool,
    monitors: Vec<MonitorInfo>,
    theme: ThemeInfo,
    recycle_bin: RecycleBinState,
    /// The platform's last power reading; `DockState::power`.
    power: PowerStatus,
    /// The platform's last volume reading; `DockState::volume`.
    volume: Option<VolumeStatus>,
    /// The platform's last reading of the regional format;
    /// `DockState::clock_format`.
    clock_format: ClockFormat,
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
            scroll_tried: BTreeMap::new(),
            own_process_id: None,
            overlap_dirty: false,
            monitors: Vec::new(),
            theme: ThemeInfo::default(),
            recycle_bin: RecycleBinState::default(),
            power: PowerStatus::default(),
            volume: None,
            clock_format: ClockFormat::default(),
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

    /// The platform's last power reading: every field `false` and
    /// no percent until the first `PlatformEvent::PowerChanged`.
    pub fn power(&self) -> PowerStatus {
        self.power
    }

    /// The platform's last reading of the default output device's volume:
    /// `None` until the first `PlatformEvent::VolumeChanged`, and
    /// while the platform knows no default output device.
    pub fn volume(&self) -> Option<VolumeStatus> {
        self.volume
    }

    /// The platform's last reading of the user's regional format:
    /// no locale and no hour cycle until the first
    /// `PlatformEvent::ClockFormatChanged`.
    pub fn clock_format(&self) -> &ClockFormat {
        &self.clock_format
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
        let foreground_before = self.foreground;
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
                    self.scroll_tried.remove(&label);
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
        if self.foreground != foreground_before {
            // The foreground moved (the report of a scroll's activation, or
            // anything else): every dock's next notch steps from it again.
            self.scroll_tried.clear();
        }
        self.rebuild_running_order();
        if self.overlap_dirty {
            self.overlap_dirty = false;
            self.compute_overlapped();
        }
        let unresolved_apps = self.take_new_unresolved();
        let changed = renders_differently(&before, &self.snapshot());
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
            // Part of the snapshot (`DockState::power`, and the battery
            // item's presence and title), so `apply` bumps the revision
            // when the reading differs from the one kept and the battery
            // item is shown, or comes or goes (`renders_differently`).
            PlatformEvent::PowerChanged(power) => self.power = power,
            // Part of the snapshot (`DockState::volume`, and the volume
            // item's title), so `apply` bumps the revision when the reading
            // differs from the one kept and the volume item is shown.
            PlatformEvent::VolumeChanged(volume) => self.volume = volume,
            // Part of the snapshot (`DockState::clock_format`) and nothing
            // else, so `apply` bumps the revision exactly when the reading
            // differs from the one kept.
            PlatformEvent::ClockFormatChanged(format) => self.clock_format = format,
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

    /// Decide what a primary click on `item_key` in the dock window `dock`
    /// should do.
    ///
    /// - fixed items map to their shell action (the volume and battery
    ///   items to quick settings, the clock to the notification center and
    ///   calendar);
    /// - an app with none of the windows that dock lists
    ///   ([`Self::windows_on_dock`]) is launched (if it can be): a pinned app
    ///   whose windows are all on other monitors starts a new one, as that
    ///   dock shows no window for it;
    /// - an app whose window is active is minimised (if configured), otherwise
    ///   activated;
    /// - an app with several windows cycles: the window after the active one,
    ///   or the first one. Only the windows that dock lists take part;
    /// - a hidden, unpinned app has no item, so a click on its key
    ///   (a page that has not seen the new state yet) does nothing.
    pub fn primary_action(&self, item_key: &str, dock: &str) -> ItemAction {
        match item_key {
            "fixed:launcher" => return ItemAction::Shell(ShellAction::OpenStartMenu),
            "fixed:task-view" => return ItemAction::Shell(ShellAction::OpenTaskView),
            "fixed:show-desktop" => return ItemAction::Shell(ShellAction::ToggleShowDesktop),
            "fixed:quick-settings" => return ItemAction::Shell(ShellAction::OpenQuickSettings),
            "fixed:notification-center" | "fixed:clock" => {
                return ItemAction::Shell(ShellAction::OpenNotificationCenter);
            }
            "fixed:volume" | "fixed:battery" => {
                return ItemAction::Shell(ShellAction::OpenQuickSettings);
            }
            "fixed:recycle-bin" => return ItemAction::Shell(ShellAction::OpenRecycleBin),
            _ => {}
        }
        let Some(app_id) = item_key.strip_prefix("app:").map(|s| AppId(s.to_owned())) else {
            return ItemAction::None;
        };
        if self.settings.is_hidden(&app_id) && !self.settings.is_pinned(&app_id) {
            return ItemAction::None;
        }
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

    /// Decide what one wheel notch `direction` over `item_key` in the dock
    /// window `dock` does (`docs/ipc/scrolling-over-an-item.md`).
    ///
    /// - a key the snapshot has no item for is [`ScrollEffect::UnknownItem`],
    ///   whatever the setting;
    /// - the volume item is [`ScrollEffect::StepVolume`] in that
    ///   direction, whatever the setting and whatever the engine's last
    ///   reading (the caller reads the volume now; without an output device
    ///   it does nothing);
    /// - with `behavior.scrollAction = cycleWindows`, an app item whose app
    ///   has windows that dock lists ([`Self::windows_on_dock`], first-seen
    ///   order, as its menu lists them) activates, for
    ///   [`ScrollDirection::Next`], the window after the one it steps from
    ///   and, for [`ScrollDirection::Previous`], the one before it,
    ///   wrapping around at either end. It steps from the window that
    ///   dock's last notch tried ([`Self::note_scroll_activation`]) while
    ///   the foreground has not changed since, that notch is less than
    ///   [`SCROLL_MEMORY`] before `now` and that window is still
    ///   one of them, else from the one in the foreground; when neither is
    ///   one of them, `Next` activates the first and `Previous` the last.
    ///   A lone window in front is its own neighbour: activated again,
    ///   which changes nothing;
    /// - anything else ([`ScrollAction::None`], another fixed item, a
    ///   separator, an app item with no window on that dock) is
    ///   [`ScrollEffect::Nothing`].
    pub fn scroll_action(
        &self,
        item_key: &str,
        dock: &str,
        direction: ScrollDirection,
        now: Instant,
    ) -> ScrollEffect {
        let Some(item) = self
            .snapshot()
            .items
            .into_iter()
            .find(|i| i.key == item_key)
        else {
            return ScrollEffect::UnknownItem;
        };
        if item.kind == DockItemKind::Volume {
            return ScrollEffect::StepVolume(direction);
        }
        if self.settings.behavior.scroll_action == ScrollAction::None {
            return ScrollEffect::Nothing;
        }
        let Some(app_id) = item.app_id.filter(|_| item.kind == DockItemKind::App) else {
            return ScrollEffect::Nothing;
        };
        let windows = self.windows_on_dock(&app_id, dock);
        let count = windows.len();
        if count == 0 {
            return ScrollEffect::Nothing;
        }
        let position = |id: WindowId| windows.iter().position(|w| *w == id);
        let from = self
            .scroll_tried
            .get(dock)
            .filter(|(_, at)| now.saturating_duration_since(*at) < SCROLL_MEMORY)
            .and_then(|(tried, _)| position(*tried))
            .or_else(|| self.foreground.and_then(position));
        let index = match (from, direction) {
            (Some(pos), ScrollDirection::Next) => (pos + 1) % count,
            (Some(pos), ScrollDirection::Previous) => (pos + count - 1) % count,
            (None, ScrollDirection::Next) => 0,
            (None, ScrollDirection::Previous) => count - 1,
        };
        ScrollEffect::Activate(windows[index])
    }

    /// The caller let a wheel notch in the dock window `dock` through at
    /// `now` and asks the platform to activate `window`. Until the
    /// foreground changes, and for at most [`SCROLL_MEMORY`] after `now`,
    /// that dock's next notch steps from `window` rather than
    /// from the window in front ([`Self::scroll_action`]): an activation
    /// Windows refuses (an elevated window, the foreground lock, a hung
    /// window) leaves the foreground where it was, and stepping from it
    /// would choose the refused window again at every notch; an
    /// activation whose foreground report has not arrived yet would be
    /// chosen twice. A notch after the limit steps from the foreground
    /// again, so it tries a refused window once more. Call it before the
    /// platform call, so the report of this very activation, or any other
    /// foreground change, ends it. Not part of the snapshot.
    pub fn note_scroll_activation(&mut self, dock: &str, window: WindowId, now: Instant) {
        self.scroll_tried.insert(dock.to_owned(), (window, now));
    }
}

/// Whether a page would show `after` differently from `before`: what
/// [`EngineOutcome::changed`] says (docs/ipc/events.md). Every
/// field counts but the revision and the two readings that only one item
/// shows: `power` counts while the battery item is shown, `volume` while the
/// volume item is. Nothing else on the pages reads them (the dock draws the
/// two faces from them; the settings page reads only `items`), so a new
/// battery percent with the battery item off, or a slider drag with the
/// volume item off, sends nothing; the snapshot carries the reading anyway,
/// so the next state sent for another reason, and the item when it is
/// switched on, show it at once. Equal `items` show the same items, so a
/// reading is compared only when both snapshots show its item, and an item
/// that comes or goes is a change of `items` itself.
fn renders_differently(before: &DockState, after: &DockState) -> bool {
    // Destructured, so that a new field of `DockState` has to be placed here.
    let DockState {
        revision: _,
        items,
        foreground,
        docks,
        power,
        volume,
        clock_format,
    } = before;
    if *items != after.items
        || *foreground != after.foreground
        || *docks != after.docks
        || *clock_format != after.clock_format
    {
        return true;
    }
    let shows = |kind: DockItemKind| items.iter().any(|i| i.kind == kind);
    (*power != after.power && shows(DockItemKind::Battery))
        || (*volume != after.volume && shows(DockItemKind::Volume))
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

/// The battery item's title and accessible name (docs/ipc/widget-items.md
/// "Clock and battery items"): `Battery 73 %`, then `, charging` while
/// charging or `, plugged in` on AC power without charging, then `, battery
/// saver on` while battery saver is on; `Battery` alone, whatever else is
/// known, while the percent is not known.
fn battery_title(power: &PowerStatus) -> String {
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

/// How far one wheel notch over the volume item moves the volume, in
/// percentage points: Windows' own step for its volume keys.
pub const VOLUME_STEP: u8 = 2;

/// What one wheel notch `direction` over the volume item makes of the
/// volume `current` (docs/ipc/widget-items.md "The volume item"):
/// [`ScrollDirection::Previous`] (the wheel turned away from the user, up)
/// raises it by [`VOLUME_STEP`], up to 100, and unmutes, as Windows'
/// volume-up key does, at 100 too; [`ScrollDirection::Next`] (towards the
/// user, down) lowers it by [`VOLUME_STEP`], down to 0, and leaves the
/// mute as it is. The level moves by the step from where it is, without
/// snapping to even numbers; a level above 100 (never sent) counts as 100.
pub fn volume_step(current: VolumeStatus, direction: ScrollDirection) -> VolumeStatus {
    let percent = current.percent.min(100);
    match direction {
        ScrollDirection::Previous => VolumeStatus {
            percent: percent.saturating_add(VOLUME_STEP).min(100),
            muted: false,
        },
        ScrollDirection::Next => VolumeStatus {
            percent: percent.saturating_sub(VOLUME_STEP),
            muted: current.muted,
        },
    }
}

/// The volume item's title and accessible name (docs/ipc/widget-items.md
/// "The volume item"): `Volume 45 %`, `Volume muted` while muted
/// (whatever the level), `No audio output` without a default output
/// device.
fn volume_title(volume: Option<&VolumeStatus>) -> String {
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

    // ----- Quick Settings and Notification Center -------------------------------

    fn with_trailing(quick_settings: bool, notification_center: bool) -> Settings {
        let mut s = Settings::default();
        s.items.quick_settings = quick_settings;
        s.items.notification_center = notification_center;
        s
    }

    #[test]
    fn quick_settings_and_notification_center_are_off_by_default() {
        let engine = DockEngine::new(Settings::default());
        let state = engine.snapshot();
        assert!(state.items.iter().all(|i| !matches!(
            i.kind,
            DockItemKind::QuickSettings | DockItemKind::NotificationCenter
        )));
    }

    #[test]
    fn quick_settings_and_notification_center_come_before_the_recycle_bin_in_that_order() {
        let mut engine = DockEngine::new(with_trailing(true, true));
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        let state = engine.snapshot();
        assert_eq!(
            keys(&state),
            vec![
                "fixed:launcher",
                "sep:1",
                "app:exe:c:/a.exe",
                "sep:2",
                "fixed:quick-settings",
                "fixed:notification-center",
                "fixed:recycle-bin",
            ]
        );
        let qs = &state.items[4];
        assert_eq!(qs.kind, DockItemKind::QuickSettings);
        assert_eq!(qs.title, "Quick Settings");
        assert_eq!(qs.icon_key, Some(IconKey("shell:quick-settings".into())));
        let nc = &state.items[5];
        assert_eq!(nc.kind, DockItemKind::NotificationCenter);
        assert_eq!(nc.title, "Notification Center");
        assert_eq!(
            nc.icon_key,
            Some(IconKey("shell:notification-center".into()))
        );
        for item in [qs, nc] {
            assert!(item.icon_key.as_ref().is_some_and(IconKey::is_shell));
            assert_eq!(item.app_id, None);
            assert!(!item.pinned);
            assert!(item.windows.is_empty());
            assert!(!item.is_active && !item.needs_attention);
        }

        // Each one alone keeps its place before the bin.
        let engine = DockEngine::new(with_trailing(true, false));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "fixed:quick-settings",
                "fixed:recycle-bin"
            ]
        );
        let engine = DockEngine::new(with_trailing(false, true));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "fixed:notification-center",
                "fixed:recycle-bin"
            ]
        );
    }

    #[test]
    fn the_trailing_group_has_one_separator_before_it_and_none_inside() {
        // Without the bin the two still close the dock, after one separator.
        let mut settings = with_trailing(true, true);
        settings.items.recycle_bin = false;
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "app:exe:c:/a.exe",
                "sep:2",
                "fixed:quick-settings",
                "fixed:notification-center",
            ]
        );

        // Alone in the dock: no separator at all.
        let mut alone = minimal_settings();
        alone.items.quick_settings = true;
        alone.items.notification_center = true;
        let engine = DockEngine::new(alone);
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["fixed:quick-settings", "fixed:notification-center"]
        );

        // With pinned and running apps: the same separators as before, the
        // last one before the whole trailing group.
        let mut full = with_trailing(true, true);
        full.pinned = vec![pin(r"C:\p.exe")];
        let mut engine = DockEngine::new(full.clone());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "app:exe:c:/p.exe",
                "sep:2",
                "app:exe:c:/a.exe",
                "sep:3",
                "fixed:quick-settings",
                "fixed:notification-center",
                "fixed:recycle-bin",
            ]
        );

        // Separators off: none anywhere.
        full.items.separators = false;
        engine.apply(EngineInput::SettingsChanged(full));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "app:exe:c:/p.exe",
                "app:exe:c:/a.exe",
                "fixed:quick-settings",
                "fixed:notification-center",
                "fixed:recycle-bin",
            ]
        );
    }

    #[test]
    fn switching_quick_settings_or_notification_center_bumps_the_revision() {
        let mut settings = Settings::default();
        let mut engine = DockEngine::new(settings.clone());
        let r = engine.snapshot().revision;

        settings.items.quick_settings = true;
        assert!(
            engine
                .apply(EngineInput::SettingsChanged(settings.clone()))
                .changed
        );
        assert!(engine.snapshot().revision > r);
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "fixed:quick-settings",
                "fixed:recycle-bin"
            ]
        );

        settings.items.notification_center = true;
        assert!(
            engine
                .apply(EngineInput::SettingsChanged(settings.clone()))
                .changed
        );
        settings.items.quick_settings = false;
        settings.items.notification_center = false;
        assert!(engine.apply(EngineInput::SettingsChanged(settings)).changed);
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
        );
    }

    #[test]
    fn a_click_on_quick_settings_or_notification_center_runs_its_shell_action() {
        let engine = DockEngine::new(with_trailing(true, true));
        assert_eq!(
            engine.primary_action("fixed:quick-settings", DOCK),
            ItemAction::Shell(ShellAction::OpenQuickSettings)
        );
        assert_eq!(
            engine.primary_action("fixed:notification-center", DOCK),
            ItemAction::Shell(ShellAction::OpenNotificationCenter)
        );
        // Any dock window, as for the other fixed items.
        assert_eq!(
            engine.primary_action("fixed:quick-settings", "dock-DISPLAY3"),
            ItemAction::Shell(ShellAction::OpenQuickSettings)
        );
        // The bin keeps its own action; the icon keys are not item keys.
        assert_eq!(
            engine.primary_action("fixed:recycle-bin", DOCK),
            ItemAction::Shell(ShellAction::OpenRecycleBin)
        );
        assert_eq!(
            engine.primary_action("shell:quick-settings", DOCK),
            ItemAction::None
        );

        // The menu is built from the item as a dock lists it: the snapshot's.
        let item = engine.dock_item("fixed:notification-center", DOCK).unwrap();
        assert_eq!(item.kind, DockItemKind::NotificationCenter);
        assert!(item.windows.is_empty());
    }

    // ----- clock and battery -------------------------------------------------

    fn with_clock_and_battery(clock: bool, battery: bool) -> Settings {
        let mut s = Settings::default();
        s.items.clock = clock;
        s.items.battery = battery;
        s
    }

    fn laptop(percent: u8) -> PowerStatus {
        PowerStatus {
            has_battery: true,
            percent: Some(percent),
            charging: false,
            on_ac: false,
            saver: false,
        }
    }

    fn power(status: PowerStatus) -> EngineInput {
        PlatformEvent::PowerChanged(status).into()
    }

    fn item<'a>(state: &'a DockState, key: &str) -> Option<&'a DockItem> {
        state.items.iter().find(|i| i.key == key)
    }

    #[test]
    fn clock_and_battery_are_off_by_default() {
        let mut engine = DockEngine::new(Settings::default());
        engine.apply(power(laptop(50)));
        let state = engine.snapshot();
        assert!(
            state
                .items
                .iter()
                .all(|i| !matches!(i.kind, DockItemKind::Clock | DockItemKind::Battery))
        );
        assert_eq!(
            keys(&state),
            vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
        );
    }

    #[test]
    fn power_is_unknown_until_the_first_reading_and_then_kept_as_sent() {
        let mut engine = DockEngine::new(with_clock_and_battery(true, true));
        assert_eq!(engine.power(), PowerStatus::default());
        let state = engine.snapshot();
        assert_eq!(state.power, PowerStatus::default());
        assert!(!state.power.has_battery);
        assert_eq!(state.power.percent, None);
        assert!(!state.power.charging && !state.power.on_ac && !state.power.saver);

        let reading = PowerStatus {
            has_battery: true,
            percent: Some(73),
            charging: true,
            on_ac: true,
            saver: true,
        };
        assert!(engine.apply(power(reading)).changed);
        assert_eq!(engine.power(), reading);
        assert_eq!(engine.snapshot().power, reading);
    }

    #[test]
    fn power_changed_bumps_the_revision_only_when_the_reading_differs() {
        let mut engine = DockEngine::new(with_clock_and_battery(true, true));
        assert!(engine.apply(power(laptop(76))).changed);
        let r = engine.snapshot().revision;

        // The same reading again (a resume, a repeated broadcast): nothing.
        assert!(!engine.apply(power(laptop(76))).changed);
        assert_eq!(engine.snapshot().revision, r);

        // Each field on its own is a change.
        let mut next = laptop(75);
        assert!(engine.apply(power(next)).changed);
        next.on_ac = true;
        assert!(engine.apply(power(next)).changed);
        next.charging = true;
        assert!(engine.apply(power(next)).changed);
        next.saver = true;
        assert!(engine.apply(power(next)).changed);
        next.percent = None;
        assert!(engine.apply(power(next)).changed);
        assert_eq!(engine.snapshot().revision, r + 5);

        // `DockState::power` is the platform's fact whatever the settings
        // say, but without the battery item no page shows it: the
        // snapshot carries each reading and the revision stays.
        let mut engine = DockEngine::new(Settings::default());
        let r = engine.snapshot().revision;
        assert!(!engine.apply(power(laptop(40))).changed);
        assert_eq!(engine.snapshot().power, laptop(40));
        assert!(!engine.apply(power(laptop(39))).changed);
        assert_eq!(engine.snapshot().power, laptop(39));
        assert_eq!(engine.snapshot().revision, r);
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
        );
    }

    #[test]
    fn battery_and_clock_come_after_notification_center_and_before_the_bin() {
        let mut settings = with_clock_and_battery(true, true);
        settings.items.quick_settings = true;
        settings.items.notification_center = true;
        let mut engine = DockEngine::new(settings);
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(power(laptop(76)));
        let state = engine.snapshot();
        assert_eq!(
            keys(&state),
            vec![
                "fixed:launcher",
                "sep:1",
                "app:exe:c:/a.exe",
                "sep:2",
                "fixed:quick-settings",
                "fixed:notification-center",
                "fixed:battery",
                "fixed:clock",
                "fixed:recycle-bin",
            ]
        );
        let battery = item(&state, "fixed:battery").unwrap();
        assert_eq!(battery.kind, DockItemKind::Battery);
        assert_eq!(battery.title, "Battery 76 %");
        assert_eq!(battery.icon_key, Some(IconKey("shell:battery".into())));
        let clock = item(&state, "fixed:clock").unwrap();
        assert_eq!(clock.kind, DockItemKind::Clock);
        assert_eq!(clock.title, "Clock");
        assert_eq!(clock.icon_key, Some(IconKey("shell:clock".into())));
        for item in [battery, clock] {
            assert!(item.icon_key.as_ref().is_some_and(IconKey::is_shell));
            assert_eq!(item.app_id, None);
            assert!(!item.pinned);
            assert!(item.windows.is_empty());
            assert!(!item.is_active && !item.needs_attention);
            assert_eq!(item.badge, None);
        }

        // Battery before clock, both before the bin, with the Quick Settings
        // and Notification Center items off.
        let mut engine = DockEngine::new(with_clock_and_battery(true, true));
        engine.apply(power(laptop(76)));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "fixed:battery",
                "fixed:clock",
                "fixed:recycle-bin",
            ]
        );
        // Each one alone keeps its place.
        let mut engine = DockEngine::new(with_clock_and_battery(false, true));
        engine.apply(power(laptop(76)));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "fixed:battery",
                "fixed:recycle-bin"
            ]
        );
        let mut settings = with_clock_and_battery(true, false);
        settings.items.notification_center = true;
        let engine = DockEngine::new(settings);
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "fixed:notification-center",
                "fixed:clock",
                "fixed:recycle-bin"
            ]
        );
    }

    #[test]
    fn the_battery_item_exists_only_while_the_system_has_a_battery() {
        let mut engine = DockEngine::new(with_clock_and_battery(true, true));
        // No reading yet: no battery is known, so no battery item.
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "fixed:clock",
                "fixed:recycle-bin"
            ]
        );

        // A desktop PC: on AC, no battery. Still no battery item.
        let desktop = PowerStatus {
            has_battery: false,
            percent: None,
            charging: false,
            on_ac: true,
            saver: false,
        };
        // No item shows the reading, so it bumps nothing.
        let r = engine.snapshot().revision;
        assert!(!engine.apply(power(desktop)).changed);
        assert!(item(&engine.snapshot(), "fixed:battery").is_none());
        assert_eq!(engine.snapshot().revision, r);
        assert_eq!(engine.snapshot().power, desktop);
        // Battery saver can be on without a battery; the item stays away.
        assert!(
            !engine
                .apply(power(PowerStatus {
                    saver: true,
                    ..desktop
                }))
                .changed
        );
        assert!(item(&engine.snapshot(), "fixed:battery").is_none());
        assert_eq!(engine.snapshot().revision, r);

        // The system sees a battery: the item appears in its place.
        assert!(engine.apply(power(laptop(30))).changed);
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "fixed:battery",
                "fixed:clock",
                "fixed:recycle-bin"
            ]
        );
        // And goes again when it no longer does.
        assert!(engine.apply(power(desktop)).changed);
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "fixed:clock",
                "fixed:recycle-bin"
            ]
        );

        // A battery with the setting off shows nothing.
        let mut engine = DockEngine::new(Settings::default());
        engine.apply(power(laptop(30)));
        assert!(item(&engine.snapshot(), "fixed:battery").is_none());

        // The battery item alone on a desktop leaves an empty trailing group:
        // no separator for it.
        let mut alone = minimal_settings();
        alone.items.battery = true;
        let mut engine = DockEngine::new(alone);
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(power(desktop));
        assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/a.exe"]);
        engine.apply(power(laptop(30)));
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["app:exe:c:/a.exe", "sep:1", "fixed:battery"]
        );
    }

    #[test]
    fn the_battery_title_says_the_percent_the_power_source_and_battery_saver() {
        let cases = [
            ((Some(73), false, false, false), "Battery 73 %"),
            ((Some(73), true, true, false), "Battery 73 %, charging"),
            ((Some(100), false, true, false), "Battery 100 %, plugged in"),
            ((Some(0), false, false, false), "Battery 0 %"),
            (
                (Some(18), false, false, true),
                "Battery 18 %, battery saver on",
            ),
            (
                (Some(18), true, true, true),
                "Battery 18 %, charging, battery saver on",
            ),
            (
                (Some(95), false, true, true),
                "Battery 95 %, plugged in, battery saver on",
            ),
            // Charging without AC (a system that reports it so) still says
            // charging.
            ((Some(50), true, false, false), "Battery 50 %, charging"),
            // Without a known percent the title is `Battery` alone, as the
            // contract says, whatever the rest of the reading is.
            ((None, false, false, false), "Battery"),
            ((None, true, true, false), "Battery"),
            ((None, false, true, false), "Battery"),
            ((None, false, false, true), "Battery"),
            ((None, true, true, true), "Battery"),
        ];
        for ((percent, charging, on_ac, saver), expected) in cases {
            let status = PowerStatus {
                has_battery: true,
                percent,
                charging,
                on_ac,
                saver,
            };
            assert_eq!(battery_title(&status), expected, "{status:?}");

            // The snapshot's item carries the same title.
            let mut engine = DockEngine::new(with_clock_and_battery(false, true));
            engine.apply(power(status));
            let state = engine.snapshot();
            assert_eq!(
                item(&state, "fixed:battery").map(|i| i.title.as_str()),
                Some(expected)
            );
        }

        // A discharge moves the title with the percent.
        let mut engine = DockEngine::new(with_clock_and_battery(false, true));
        engine.apply(power(laptop(76)));
        engine.apply(power(laptop(75)));
        assert_eq!(
            item(&engine.snapshot(), "fixed:battery").unwrap().title,
            "Battery 75 %"
        );
    }

    #[test]
    fn the_clock_and_battery_follow_the_trailing_groups_separator_rules() {
        // Without the bin they close the dock, after one separator.
        let mut settings = with_clock_and_battery(true, true);
        settings.items.recycle_bin = false;
        let mut engine = DockEngine::new(settings);
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(power(laptop(60)));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "sep:1",
                "app:exe:c:/a.exe",
                "sep:2",
                "fixed:battery",
                "fixed:clock",
            ]
        );

        // Alone in the dock: no separator at all.
        let mut alone = minimal_settings();
        alone.items.clock = true;
        alone.items.battery = true;
        let mut engine = DockEngine::new(alone.clone());
        engine.apply(power(laptop(60)));
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["fixed:battery", "fixed:clock"]
        );

        // Separators off: none anywhere.
        let mut full = with_clock_and_battery(true, true);
        full.items.separators = false;
        let mut engine = DockEngine::new(full);
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(power(laptop(60)));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "fixed:launcher",
                "app:exe:c:/a.exe",
                "fixed:battery",
                "fixed:clock",
                "fixed:recycle-bin",
            ]
        );
    }

    #[test]
    fn switching_the_clock_or_the_battery_bumps_the_revision_only_when_it_shows() {
        let mut settings = Settings::default();
        let mut engine = DockEngine::new(settings.clone());
        let r = engine.snapshot().revision;

        // The battery item on a machine with no battery (no reading yet)
        // changes no item: no bump.
        settings.items.battery = true;
        assert!(
            !engine
                .apply(EngineInput::SettingsChanged(settings.clone()))
                .changed
        );
        assert_eq!(engine.snapshot().revision, r);

        // The reading shows it.
        assert!(engine.apply(power(laptop(80))).changed);
        assert!(item(&engine.snapshot(), "fixed:battery").is_some());

        settings.items.clock = true;
        assert!(
            engine
                .apply(EngineInput::SettingsChanged(settings.clone()))
                .changed
        );
        settings.items.battery = false;
        settings.items.clock = false;
        assert!(engine.apply(EngineInput::SettingsChanged(settings)).changed);
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
        );
        // The reading is kept across setting changes.
        assert_eq!(engine.power(), laptop(80));
    }

    #[test]
    fn a_click_on_the_clock_or_the_battery_opens_its_windows_panel() {
        let mut engine = DockEngine::new(with_clock_and_battery(true, true));
        engine.apply(power(laptop(50)));
        assert_eq!(
            engine.primary_action("fixed:clock", DOCK),
            ItemAction::Shell(ShellAction::OpenNotificationCenter)
        );
        assert_eq!(
            engine.primary_action("fixed:battery", DOCK),
            ItemAction::Shell(ShellAction::OpenQuickSettings)
        );
        // Any dock window, as for the other fixed items.
        assert_eq!(
            engine.primary_action("fixed:clock", "dock-DISPLAY2"),
            ItemAction::Shell(ShellAction::OpenNotificationCenter)
        );
        assert_eq!(
            engine.primary_action("fixed:battery", "dock-DISPLAY2"),
            ItemAction::Shell(ShellAction::OpenQuickSettings)
        );
        // The icon keys are not item keys.
        assert_eq!(engine.primary_action("shell:clock", DOCK), ItemAction::None);
        assert_eq!(
            engine.primary_action("shell:battery", DOCK),
            ItemAction::None
        );

        // The menus are built from the items as a dock lists them.
        let clock = engine.dock_item("fixed:clock", DOCK).unwrap();
        assert_eq!(clock.kind, DockItemKind::Clock);
        let battery = engine.dock_item("fixed:battery", DOCK).unwrap();
        assert_eq!(battery.kind, DockItemKind::Battery);
        assert_eq!(battery.title, "Battery 50 %");

        // Without a battery there is no battery item to build a menu from.
        engine.apply(power(PowerStatus::default()));
        assert!(engine.dock_item("fixed:battery", DOCK).is_none());
    }

    // ----- the volume item --------------------------------------------------------

    fn with_volume(on: bool) -> Settings {
        let mut s = Settings::default();
        s.items.volume = on;
        s
    }

    fn level(percent: u8, muted: bool) -> VolumeStatus {
        VolumeStatus { percent, muted }
    }

    fn volume(status: Option<VolumeStatus>) -> EngineInput {
        PlatformEvent::VolumeChanged(status).into()
    }

    #[test]
    fn the_volume_item_is_off_by_default_and_its_reading_is_kept_whatever_the_settings() {
        let mut engine = DockEngine::new(Settings::default());
        assert_eq!(engine.volume(), None);
        assert_eq!(engine.snapshot().volume, None);
        // The first reading reaches the snapshot even with the item off,
        // without a new revision, as no item shows it.
        assert!(!engine.apply(volume(Some(level(60, false)))).changed);
        assert_eq!(engine.volume(), Some(level(60, false)));
        assert_eq!(engine.snapshot().volume, Some(level(60, false)));
        assert!(
            engine
                .snapshot()
                .items
                .iter()
                .all(|i| i.kind != DockItemKind::Volume)
        );
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["fixed:launcher", "sep:1", "fixed:recycle-bin"]
        );
    }

    #[test]
    fn volume_changed_bumps_the_revision_only_when_the_reading_differs() {
        let mut engine = DockEngine::new(with_volume(true));
        let r = engine.snapshot().revision;
        // "No device" before the first reading: the same again is nothing.
        assert!(!engine.apply(volume(None)).changed);
        assert_eq!(engine.snapshot().revision, r);

        assert!(engine.apply(volume(Some(level(60, false)))).changed);
        assert!(!engine.apply(volume(Some(level(60, false)))).changed);
        assert!(engine.apply(volume(Some(level(62, false)))).changed);
        assert!(engine.apply(volume(Some(level(62, true)))).changed);
        assert!(engine.apply(volume(None)).changed);
        assert!(!engine.apply(volume(None)).changed);
        assert_eq!(engine.snapshot().revision, r + 4);

        // The item off: still part of the snapshot, but no item shows it,
        // so no bump.
        let mut engine = DockEngine::new(Settings::default());
        let r = engine.snapshot().revision;
        assert!(!engine.apply(volume(Some(level(10, false)))).changed);
        assert!(!engine.apply(volume(Some(level(10, true)))).changed);
        assert!(!engine.apply(volume(None)).changed);
        assert_eq!(engine.snapshot().volume, None);
        assert_eq!(engine.snapshot().revision, r);
    }

    #[test]
    fn a_reading_no_shown_item_uses_is_kept_without_a_new_revision() {
        // The battery and the volume item off, readings come in.
        let mut engine = DockEngine::new(Settings::default());
        let r = engine.snapshot().revision;
        for percent in [76, 75, 74] {
            assert!(!engine.apply(power(laptop(percent))).changed);
        }
        for percent in [60, 61, 62] {
            assert!(!engine.apply(volume(Some(level(percent, false)))).changed);
        }
        assert_eq!(engine.snapshot().revision, r);
        // The next state sent for another reason carries the latest of both.
        assert!(engine.apply(added(win(1, r"C:\a.exe", "A"))).changed);
        let state = engine.snapshot();
        assert_eq!(state.revision, r + 1);
        assert_eq!(state.power, laptop(74));
        assert_eq!(state.volume, Some(level(62, false)));

        // An item switched on shows the reading kept at once.
        let mut settings = engine.settings().clone();
        settings.items.battery = true;
        settings.items.volume = true;
        assert!(
            engine
                .apply(EngineInput::SettingsChanged(settings.clone()))
                .changed
        );
        let state = engine.snapshot();
        assert_eq!(item(&state, "fixed:battery").unwrap().title, "Battery 74 %");
        assert_eq!(item(&state, "fixed:volume").unwrap().title, "Volume 62 %");
        // While they are shown, every new reading of either is a change.
        let r = state.revision;
        assert!(engine.apply(power(laptop(73))).changed);
        assert!(engine.apply(volume(Some(level(62, true)))).changed);
        assert_eq!(engine.snapshot().revision, r + 2);

        // Each one counts on its own item only: the volume item off, a new
        // volume reading is nothing while the battery item still shows the
        // power, and the other way round.
        settings.items.volume = false;
        assert!(
            engine
                .apply(EngineInput::SettingsChanged(settings.clone()))
                .changed
        );
        assert!(!engine.apply(volume(Some(level(30, false)))).changed);
        assert!(engine.apply(power(laptop(72))).changed);
        settings.items.volume = true;
        settings.items.battery = false;
        assert!(
            engine
                .apply(EngineInput::SettingsChanged(settings.clone()))
                .changed
        );
        assert_eq!(
            item(&engine.snapshot(), "fixed:volume").unwrap().title,
            "Volume 30 %"
        );
        assert!(!engine.apply(power(laptop(71))).changed);
        assert!(engine.apply(volume(Some(level(31, false)))).changed);

        // The battery item on but no system battery: no item shows the
        // power, so battery saver toggling on a desktop is nothing either,
        // and a battery that appears is a change (its item comes).
        settings.items.battery = true;
        let desktop = PowerStatus {
            has_battery: false,
            percent: None,
            charging: false,
            on_ac: true,
            saver: false,
        };
        assert!(!engine.apply(power(desktop)).changed);
        assert!(!engine.apply(EngineInput::SettingsChanged(settings)).changed);
        assert!(item(&engine.snapshot(), "fixed:battery").is_none());
        let r = engine.snapshot().revision;
        assert!(
            !engine
                .apply(power(PowerStatus {
                    saver: true,
                    ..desktop
                }))
                .changed
        );
        assert_eq!(engine.snapshot().revision, r);
        assert!(engine.apply(power(laptop(70))).changed);
        assert_eq!(engine.snapshot().revision, r + 1);
        assert!(item(&engine.snapshot(), "fixed:battery").is_some());
        // And one that goes is a change too (its item goes).
        assert!(engine.apply(power(desktop)).changed);
        assert!(item(&engine.snapshot(), "fixed:battery").is_none());
    }

    #[test]
    fn the_volume_item_comes_after_notification_center_and_before_the_battery() {
        let mut settings = with_clock_and_battery(true, true);
        settings.items.quick_settings = true;
        settings.items.notification_center = true;
        settings.items.volume = true;
        let mut engine = DockEngine::new(settings);
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(power(laptop(76)));
        engine.apply(volume(Some(level(45, false))));
        let state = engine.snapshot();
        assert_eq!(
            keys(&state),
            vec![
                "fixed:launcher",
                "sep:1",
                "app:exe:c:/a.exe",
                "sep:2",
                "fixed:quick-settings",
                "fixed:notification-center",
                "fixed:volume",
                "fixed:battery",
                "fixed:clock",
                "fixed:recycle-bin",
            ]
        );
        let volume_item = item(&state, "fixed:volume").unwrap();
        assert_eq!(volume_item.kind, DockItemKind::Volume);
        assert_eq!(volume_item.title, "Volume 45 %");
        assert_eq!(volume_item.icon_key, Some(IconKey("shell:volume".into())));
        assert_eq!(volume_item.app_id, None);
        assert!(!volume_item.pinned && volume_item.windows.is_empty());
        assert!(!volume_item.is_active && !volume_item.needs_attention);

        // Shown without a device too (its face says so), alone in its
        // group after one separator, and closing the dock without the bin.
        let mut alone = with_volume(true);
        alone.items.recycle_bin = false;
        assert_eq!(
            keys(&DockEngine::new(alone).snapshot()),
            vec!["fixed:launcher", "sep:1", "fixed:volume"]
        );
        let mut only = minimal_settings();
        only.items.volume = true;
        assert_eq!(
            keys(&DockEngine::new(only).snapshot()),
            vec!["fixed:volume"]
        );
    }

    #[test]
    fn the_volume_title_says_the_level_muted_or_no_output() {
        for (reading, expected) in [
            (Some(level(45, false)), "Volume 45 %"),
            (Some(level(0, false)), "Volume 0 %"),
            (Some(level(100, false)), "Volume 100 %"),
            (Some(level(45, true)), "Volume muted"),
            (Some(level(0, true)), "Volume muted"),
            (None, "No audio output"),
        ] {
            assert_eq!(volume_title(reading.as_ref()), expected, "{reading:?}");
            let mut engine = DockEngine::new(with_volume(true));
            engine.apply(volume(reading));
            assert_eq!(
                item(&engine.snapshot(), "fixed:volume").map(|i| i.title.as_str()),
                Some(expected)
            );
        }
    }

    #[test]
    fn switching_the_volume_item_bumps_the_revision_and_keeps_the_reading() {
        let mut settings = Settings::default();
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(volume(Some(level(30, false))));
        let r = engine.snapshot().revision;
        settings.items.volume = true;
        assert!(
            engine
                .apply(EngineInput::SettingsChanged(settings.clone()))
                .changed
        );
        assert_eq!(engine.snapshot().revision, r + 1);
        settings.items.volume = false;
        assert!(engine.apply(EngineInput::SettingsChanged(settings)).changed);
        assert_eq!(engine.volume(), Some(level(30, false)));
    }

    #[test]
    fn a_click_on_the_volume_item_opens_quick_settings() {
        let mut engine = DockEngine::new(with_volume(true));
        for dock in [DOCK, "dock-DISPLAY2"] {
            assert_eq!(
                engine.primary_action("fixed:volume", dock),
                ItemAction::Shell(ShellAction::OpenQuickSettings)
            );
        }
        engine.apply(volume(Some(level(20, true))));
        assert_eq!(
            engine.primary_action("fixed:volume", DOCK),
            ItemAction::Shell(ShellAction::OpenQuickSettings)
        );
        assert_eq!(
            engine.primary_action("shell:volume", DOCK),
            ItemAction::None
        );
        // Its menu is built from the item as the dock lists it.
        let menu_item = engine.dock_item("fixed:volume", DOCK).unwrap();
        assert_eq!(menu_item.kind, DockItemKind::Volume);
        assert_eq!(menu_item.title, "Volume muted");
    }

    #[test]
    fn a_scroll_over_the_volume_item_steps_the_volume_whatever_the_scroll_action() {
        use ScrollDirection::{Next, Previous};
        let mut settings = with_volume(true);
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(volume(Some(level(60, false))));
        // `none`, the default: the volume still steps, other items do not.
        assert_eq!(settings.behavior.scroll_action, ScrollAction::None);
        for direction in [Next, Previous] {
            assert_eq!(
                engine.scroll_action("fixed:volume", DOCK, direction, t0()),
                ScrollEffect::StepVolume(direction)
            );
            assert_eq!(
                engine.scroll_action("fixed:launcher", DOCK, direction, t0()),
                ScrollEffect::Nothing
            );
        }
        // `cycleWindows`: the same, on any dock window.
        settings = cycling(settings);
        engine.apply(EngineInput::SettingsChanged(settings.clone()));
        assert_eq!(
            engine.scroll_action("fixed:volume", "dock-DISPLAY2", Previous, t0()),
            ScrollEffect::StepVolume(Previous)
        );
        // Without a device the caller finds out when it reads the volume.
        engine.apply(volume(None));
        assert_eq!(
            engine.scroll_action("fixed:volume", DOCK, Next, t0()),
            ScrollEffect::StepVolume(Next)
        );
        // The item off: its key is unknown.
        settings.items.volume = false;
        engine.apply(EngineInput::SettingsChanged(settings));
        assert_eq!(
            engine.scroll_action("fixed:volume", DOCK, Next, t0()),
            ScrollEffect::UnknownItem
        );
    }

    #[test]
    fn a_volume_step_moves_two_points_clamps_and_unmutes_going_up() {
        use ScrollDirection::{Next, Previous};
        assert_eq!(VOLUME_STEP, 2);
        for (from, direction, to) in [
            // Up: two points, unmuted, never past 100.
            (level(45, false), Previous, level(47, false)),
            (level(98, false), Previous, level(100, false)),
            (level(99, false), Previous, level(100, false)),
            (level(100, false), Previous, level(100, false)),
            (level(45, true), Previous, level(47, false)),
            (level(100, true), Previous, level(100, false)),
            (level(0, true), Previous, level(2, false)),
            // Down: two points, the mute kept, never below 0.
            (level(45, false), Next, level(43, false)),
            (level(2, false), Next, level(0, false)),
            (level(1, false), Next, level(0, false)),
            (level(0, false), Next, level(0, false)),
            (level(45, true), Next, level(43, true)),
            (level(0, true), Next, level(0, true)),
            // A reading out of range (never sent) counts as 100.
            (level(250, false), Next, level(98, false)),
            (level(250, false), Previous, level(100, false)),
        ] {
            assert_eq!(volume_step(from, direction), to, "{from:?} {direction:?}");
        }
        // Notches up from 0 reach 100 and stay there.
        let mut v = level(0, false);
        for _ in 0..60 {
            v = volume_step(v, Previous);
        }
        assert_eq!(v, level(100, false));
    }

    // ----- the clock's regional format ----------------------------------------

    fn clock_format(locale: Option<&str>, hour12: Option<bool>) -> ClockFormat {
        ClockFormat {
            locale: locale.map(str::to_owned),
            hour12,
        }
    }

    fn format_changed(format: ClockFormat) -> EngineInput {
        PlatformEvent::ClockFormatChanged(format).into()
    }

    #[test]
    fn clock_format_is_unknown_until_the_first_reading_and_then_kept_as_sent() {
        let mut engine = DockEngine::new(with_clock_and_battery(true, false));
        assert_eq!(engine.clock_format(), &ClockFormat::default());
        let state = engine.snapshot();
        assert_eq!(state.clock_format.locale, None);
        assert_eq!(state.clock_format.hour12, None);

        // The first reading equal to "nothing known" changes nothing.
        let r = state.revision;
        assert!(!engine.apply(format_changed(ClockFormat::default())).changed);
        assert_eq!(engine.snapshot().revision, r);

        let british = clock_format(Some("en-GB"), Some(false));
        assert!(engine.apply(format_changed(british.clone())).changed);
        assert_eq!(engine.clock_format(), &british);
        assert_eq!(engine.snapshot().clock_format, british);
        // Stored as is: the engine does not judge the name.
        let odd = clock_format(Some("not a locale"), None);
        assert!(engine.apply(format_changed(odd.clone())).changed);
        assert_eq!(engine.snapshot().clock_format, odd);
    }

    #[test]
    fn clock_format_changed_bumps_the_revision_only_when_the_reading_differs() {
        let mut engine = DockEngine::new(with_clock_and_battery(true, false));
        let american = clock_format(Some("en-US"), Some(true));
        assert!(engine.apply(format_changed(american.clone())).changed);
        let r = engine.snapshot().revision;

        // The same reading again (another `intl` broadcast): nothing.
        assert!(!engine.apply(format_changed(american)).changed);
        assert_eq!(engine.snapshot().revision, r);

        // Each field on its own is a change: a custom 24-hour short time in
        // the same region, then another region, then a reading that says
        // less.
        assert!(
            engine
                .apply(format_changed(clock_format(Some("en-US"), Some(false))))
                .changed
        );
        assert!(
            engine
                .apply(format_changed(clock_format(Some("de-DE"), Some(false))))
                .changed
        );
        assert!(
            engine
                .apply(format_changed(clock_format(Some("de-DE"), None)))
                .changed
        );
        assert!(
            engine
                .apply(format_changed(clock_format(None, None)))
                .changed
        );
        assert_eq!(engine.snapshot().revision, r + 4);

        // `DockState::clock_format` is the platform's fact whatever the
        // settings say: without the clock a change still reaches the page,
        // and no item moves.
        let mut engine = DockEngine::new(Settings::default());
        let before = engine.snapshot().items;
        assert!(
            engine
                .apply(format_changed(clock_format(Some("fr-FR"), Some(false))))
                .changed
        );
        assert_eq!(
            engine.snapshot().clock_format.locale.as_deref(),
            Some("fr-FR")
        );
        assert_eq!(engine.snapshot().items, before);
    }

    #[test]
    fn the_clock_item_is_the_same_in_any_regional_format() {
        let mut settings = with_clock_and_battery(true, true);
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(power(laptop(60)));
        let items = engine.snapshot().items;
        let clock = item(&engine.snapshot(), "fixed:clock").cloned().unwrap();
        assert_eq!(clock.title, "Clock");

        // The page formats the time; Rust's items do not change with it.
        engine.apply(format_changed(clock_format(Some("ko-KR"), Some(true))));
        assert_eq!(engine.snapshot().items, items);
        assert_eq!(
            engine.primary_action("fixed:clock", DOCK),
            ItemAction::Shell(ShellAction::OpenNotificationCenter)
        );

        // The reading is kept across setting changes, the clock's own too.
        settings.items.clock = false;
        assert!(
            engine
                .apply(EngineInput::SettingsChanged(settings.clone()))
                .changed
        );
        settings.items.clock = true;
        engine.apply(EngineInput::SettingsChanged(settings));
        assert_eq!(
            engine.clock_format(),
            &clock_format(Some("ko-KR"), Some(true))
        );
        assert_eq!(item(&engine.snapshot(), "fixed:clock"), Some(&clock));
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

    // ----- hidden apps -------------------------------------------------------------

    fn hidden(exe: &str, name: &str) -> crate::settings::HiddenApp {
        crate::settings::HiddenApp {
            id: exe_id(exe),
            name: name.into(),
            path: None,
            icon_key: None,
        }
    }

    #[test]
    fn a_hidden_app_has_no_item_and_comes_back_when_shown() {
        let mut settings = minimal_settings();
        settings.pinned = vec![pin(r"C:\p.exe")];
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(added(win(1, r"C:\a.exe", "A")));
        engine.apply(added(win(2, r"C:\b.exe", "B")));
        engine.apply(added(win(3, r"C:\a.exe", "A2")));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "app:exe:c:/p.exe",
                "sep:1",
                "app:exe:c:/a.exe",
                "app:exe:c:/b.exe"
            ]
        );

        // Hide A: its item goes (one bump), B keeps its place.
        settings.hidden_apps = vec![hidden(r"C:\a.exe", "A")];
        let r = engine.snapshot().revision;
        let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
        assert!(out.changed);
        assert_eq!(engine.snapshot().revision, r + 1);
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["app:exe:c:/p.exe", "sep:1", "app:exe:c:/b.exe"]
        );

        // A second window of A brings no item either, and changes nothing
        // the snapshot shows.
        let out = engine.apply(added(win(4, r"C:\a.exe", "A3")));
        assert!(!out.changed);
        assert_eq!(
            keys(&engine.snapshot()),
            vec!["app:exe:c:/p.exe", "sep:1", "app:exe:c:/b.exe"]
        );

        // With B hidden too, no running group is left and so no separator.
        settings.hidden_apps.push(hidden(r"C:\b.exe", "B"));
        engine.apply(EngineInput::SettingsChanged(settings.clone()));
        assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/p.exe"]);

        // Shown again: A is back where first-seen order puts it, with all
        // its windows, the one added while hidden included.
        settings.hidden_apps = vec![hidden(r"C:\b.exe", "B")];
        assert!(
            engine
                .apply(EngineInput::SettingsChanged(settings.clone()))
                .changed
        );
        let state = engine.snapshot();
        assert_eq!(
            keys(&state),
            vec!["app:exe:c:/p.exe", "sep:1", "app:exe:c:/a.exe"]
        );
        assert_eq!(ids(&state.items[2].windows), vec![1, 3, 4]);
        settings.hidden_apps.clear();
        engine.apply(EngineInput::SettingsChanged(settings));
        assert_eq!(
            keys(&engine.snapshot()),
            vec![
                "app:exe:c:/p.exe",
                "sep:1",
                "app:exe:c:/a.exe",
                "app:exe:c:/b.exe"
            ]
        );
    }

    #[test]
    fn a_hidden_apps_windows_stay_tracked() {
        let mut settings = minimal_settings();
        settings.hidden_apps = vec![hidden(r"C:\a.exe", "A")];
        let mut engine = DockEngine::new(settings);
        engine.apply(monitors(vec![monitor(PRIMARY, true, 0)]));
        engine.apply(panel(Some(PANEL)));
        engine.apply(added(framed(1, r"C:\a.exe", OVER)));
        engine.apply(added(win(2, r"C:\b.exe", "B")));
        let a = exe_id(r"C:\a.exe");

        // No item, but the window is known: its frame over the panel
        // overlaps the dock, and it can be the foreground window, with no
        // item active for it.
        assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/b.exe"]);
        assert!(engine.window(WindowId(1)).is_some());
        assert_eq!(engine.windows_of(&a), vec![WindowId(1)]);
        assert!(is_overlapped(&engine, PRIMARY));
        assert!(engine.apply(foreground(Some(1))).changed);
        let state = engine.snapshot();
        assert_eq!(state.foreground, Some(WindowId(1)));
        assert!(state.items.iter().all(|i| !i.is_active));

        // Fullscreen is the platform's verdict, whoever's window it is.
        engine.apply(fullscreen_on(Some(PRIMARY)));
        assert!(is_fullscreen(&engine, PRIMARY));

        // Its identity is still asked for, and a click on its key (from a
        // page a state behind) does nothing.
        assert!(engine.unresolved_apps().contains(&a));
        assert_eq!(
            engine.primary_action("app:exe:c:/a.exe", DOCK),
            ItemAction::None
        );
        assert_eq!(engine.dock_item("app:exe:c:/a.exe", DOCK), None);

        // Its window closing changes the overlap like any other.
        engine.apply(removed(1));
        assert!(!is_overlapped(&engine, PRIMARY));
        assert_eq!(engine.snapshot().foreground, None);
    }

    #[test]
    fn a_pinned_app_is_never_hidden_even_by_an_unvalidated_list() {
        let mut settings = minimal_settings();
        settings.pinned = vec![pin(r"C:\p.exe")];
        // Not validated: the engine itself keeps the pin.
        settings.hidden_apps = vec![hidden(r"C:\p.exe", "P")];
        let mut engine = DockEngine::new(settings);
        engine.apply(added(win(1, r"C:\p.exe", "P")));
        let state = engine.snapshot();
        assert_eq!(keys(&state), vec!["app:exe:c:/p.exe"]);
        assert!(state.items[0].pinned);
        assert_eq!(ids(&state.items[0].windows), vec![1]);
        assert_eq!(
            engine.primary_action("app:exe:c:/p.exe", DOCK),
            ItemAction::ActivateWindow(WindowId(1))
        );
    }

    #[test]
    fn a_hidden_app_has_no_item_on_any_dock() {
        let mut engine = two_docks(false, true);
        let mut settings = engine.settings().clone();
        settings.hidden_apps = vec![hidden(NOTEPAD, "Notepad")];
        engine.apply(EngineInput::SettingsChanged(settings));
        for dock in [DOCK, SECOND_DOCK] {
            assert_eq!(engine.dock_item(NOTEPAD_KEY, dock), None, "{dock}");
            assert_eq!(
                engine.primary_action(NOTEPAD_KEY, dock),
                ItemAction::None,
                "{dock}"
            );
        }
        assert!(engine.snapshot().items.is_empty());
        // Still tracked on both monitors.
        assert_eq!(
            engine.windows_on_dock(&exe_id(NOTEPAD), SECOND_DOCK),
            vec![WindowId(2)]
        );
    }

    #[test]
    fn app_name_is_the_identity_else_the_first_windows_title() {
        let mut engine = DockEngine::new(minimal_settings());
        let a = exe_id(r"C:\a.exe");
        assert_eq!(engine.app_name(&a), None, "unknown app");
        engine.apply(added(cloaked(win(4, r"C:\a.exe", "first, cloaked"))));
        // No shown window, so no item: the first-seen window names it.
        assert_eq!(engine.app_name(&a).as_deref(), Some("first, cloaked"));
        engine.apply(added(win(3, r"C:\a.exe", "second")));
        engine.apply(added(win(2, r"C:\a.exe", "third")));
        // The title the item shows: its first shown window's, in first-seen
        // order (a lower window id does not matter), not the cloaked one's.
        let title = |engine: &DockEngine| {
            engine
                .snapshot()
                .items
                .into_iter()
                .find(|i| i.app_id.as_ref() == Some(&a))
                .map(|i| i.title)
        };
        assert_eq!(title(&engine).as_deref(), Some("second"));
        assert_eq!(engine.app_name(&a), title(&engine));
        engine.apply(removed(3));
        assert_eq!(engine.app_name(&a).as_deref(), Some("third"));
        assert_eq!(engine.app_name(&a), title(&engine));
        engine.apply(EngineInput::IdentityResolved {
            app_id: a.clone(),
            identity: identity(&a, "App A", "icon:a"),
        });
        assert_eq!(engine.app_name(&a).as_deref(), Some("App A"));
        assert_eq!(engine.app_name(&a), title(&engine));
        // The identity outlives the windows.
        engine.apply(removed(2));
        engine.apply(removed(4));
        assert_eq!(engine.app_name(&a).as_deref(), Some("App A"));
    }

    #[test]
    fn app_window_is_the_first_shown_window_else_the_first_seen_one() {
        let mut engine = DockEngine::new(minimal_settings());
        let a = exe_id(r"C:\a.exe");
        assert_eq!(engine.app_window(&a), None, "no window");
        engine.apply(added(cloaked(win(4, r"C:\a.exe", "first, cloaked"))));
        engine.apply(added(cloaked(win(5, r"C:\a.exe", "second, cloaked"))));
        assert_eq!(engine.app_window(&a).map(|w| w.id), Some(WindowId(4)));
        engine.apply(added(win(3, r"C:\a.exe", "shown")));
        assert_eq!(engine.app_window(&a).map(|w| w.id), Some(WindowId(3)));
        assert_eq!(
            engine.app_window(&a).and_then(|w| w.exe_path.clone()),
            Some(std::path::PathBuf::from(r"C:\a.exe"))
        );
        // An identity does not stand in for a window.
        engine.apply(EngineInput::IdentityResolved {
            app_id: a.clone(),
            identity: identity(&a, "App A", "icon:a"),
        });
        for id in [3, 4, 5] {
            engine.apply(removed(id));
        }
        assert_eq!(engine.app_window(&a), None);
        assert_eq!(engine.app_name(&a).as_deref(), Some("App A"));
    }

    // ----- scrolling over an item ---------------------------------------------------

    const A_KEY: &str = "app:exe:c:/a.exe";

    fn cycling(mut settings: Settings) -> Settings {
        settings.behavior.scroll_action = ScrollAction::CycleWindows;
        settings
    }

    fn activates(id: u64) -> ScrollEffect {
        ScrollEffect::Activate(WindowId(id))
    }

    /// One instant for every scroll test that does not look at the time:
    /// a memory noted at it never runs out at it.
    fn t0() -> Instant {
        static T0: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);
        *T0
    }

    #[test]
    fn a_scroll_cycles_the_items_windows_from_the_one_in_front_and_wraps() {
        use ScrollDirection::{Next, Previous};
        let mut engine = DockEngine::new(cycling(minimal_settings()));
        engine.apply(added(win(1, r"C:\a.exe", "A1")));
        let mut minimised = win(2, r"C:\a.exe", "A2");
        minimised.is_minimized = true;
        engine.apply(added(minimised));
        engine.apply(added(cloaked(win(9, r"C:\a.exe", "on another desktop"))));
        engine.apply(added(win(3, r"C:\a.exe", "A3")));
        engine.apply(added(win(4, r"C:\b.exe", "B")));

        // None of A's windows in front: next is the first, previous the last.
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Previous, t0()),
            activates(3)
        );
        engine.apply(foreground(Some(4)));
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Previous, t0()),
            activates(3)
        );

        // From each window in front, its neighbours in first-seen order; the
        // minimised one takes part, the cloaked one does not.
        for (front, next, previous) in [(1, 2, 3), (2, 3, 1), (3, 1, 2)] {
            engine.apply(foreground(Some(front)));
            assert_eq!(
                engine.scroll_action(A_KEY, DOCK, Next, t0()),
                activates(next),
                "next from {front}"
            );
            assert_eq!(
                engine.scroll_action(A_KEY, DOCK, Previous, t0()),
                activates(previous),
                "previous from {front}"
            );
        }

        // A lone window in front is its own neighbour either way.
        engine.apply(foreground(Some(4)));
        for direction in [Next, Previous] {
            assert_eq!(
                engine.scroll_action("app:exe:c:/b.exe", DOCK, direction, t0()),
                activates(4)
            );
        }
    }

    #[test]
    fn a_scroll_steps_from_the_window_the_dock_last_tried_until_the_foreground_moves() {
        use ScrollDirection::{Next, Previous};
        let mut engine = DockEngine::new(cycling(minimal_settings()));
        for id in 1..=3 {
            engine.apply(added(win(id, r"C:\a.exe", &format!("A{id}"))));
        }
        engine.apply(added(win(4, r"C:\b.exe", "B")));
        engine.apply(foreground(Some(1)));

        // Windows refuses 2 (an elevated window, say): the foreground stays
        // on 1, and the next notch goes on to 3 instead of trying 2 again;
        // back from 2 is 1.
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(2));
        engine.note_scroll_activation(DOCK, WindowId(2), t0());
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(3));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Previous, t0()),
            activates(1)
        );
        // 3 is being activated and its report is not in yet: from 3.
        engine.note_scroll_activation(DOCK, WindowId(3), t0());
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Previous, t0()),
            activates(2)
        );
        // Each dock window has its own: another steps from the foreground.
        assert_eq!(
            engine.scroll_action(A_KEY, SECOND_DOCK, Next, t0()),
            activates(2)
        );
        // A report of the window that already was in front changes nothing.
        engine.apply(foreground(Some(1)));
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
        // 3's report: from the foreground again, which is 3.
        engine.apply(foreground(Some(3)));
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Previous, t0()),
            activates(2)
        );

        // Any change of the foreground ends it, a change and a change back
        // too (the user clicked elsewhere and returned).
        engine.note_scroll_activation(DOCK, WindowId(1), t0());
        engine.apply(foreground(Some(4)));
        engine.apply(foreground(Some(3)));
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
        // So does the foreground window's going away.
        engine.note_scroll_activation(DOCK, WindowId(1), t0());
        engine.apply(removed(3));
        assert_eq!(engine.foreground(), None);
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));

        // A tried window that is not one of the item's windows on that dock
        // (another app's, or one closed since) is passed over: from the
        // foreground, or the first when none is in front.
        engine.apply(added(win(5, r"C:\a.exe", "A5")));
        engine.apply(foreground(Some(2)));
        engine.note_scroll_activation(DOCK, WindowId(4), t0());
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(5));
        engine.note_scroll_activation(DOCK, WindowId(1), t0());
        engine.apply(removed(1));
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(5));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Previous, t0()),
            activates(5)
        );

        // A dock window that goes takes its entry with it.
        engine.apply(panel_of(DOCK, Some(PRIMARY), None));
        engine.note_scroll_activation(DOCK, WindowId(5), t0());
        engine.note_scroll_activation(SECOND_DOCK, WindowId(5), t0());
        engine.apply(panel_of(DOCK, None, None));
        assert_eq!(
            engine.scroll_tried.keys().collect::<Vec<_>>(),
            [SECOND_DOCK]
        );
    }

    #[test]
    fn the_window_a_scroll_tried_is_forgotten_after_the_scroll_memory() {
        use ScrollDirection::{Next, Previous};
        assert_eq!(SCROLL_MEMORY, Duration::from_secs(2));
        let mut engine = DockEngine::new(cycling(minimal_settings()));
        for id in 1..=3 {
            engine.apply(added(win(id, r"C:\a.exe", &format!("A{id}"))));
        }
        engine.apply(foreground(Some(1)));
        let at = |ms: u64| t0() + Duration::from_millis(ms);

        // Windows refuses 2: the foreground stays on 1. Within the memory
        // the dock's next notches step from 2, so 2 is passed over.
        engine.note_scroll_activation(DOCK, WindowId(2), at(0));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Next, at(150)),
            activates(3)
        );
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Previous, at(1_999)),
            activates(1)
        );
        // A notch at the limit or later steps from the foreground again and
        // tries 2 once more.
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Next, at(2_000)),
            activates(2)
        );
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Previous, at(60_000)),
            activates(3)
        );
        // The memory runs from the last admitted notch: a new one starts it
        // again.
        engine.note_scroll_activation(DOCK, WindowId(2), at(2_000));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Next, at(3_999)),
            activates(3)
        );
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Next, at(4_000)),
            activates(2)
        );
        // A time before the notch (never sent) counts as within it.
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Next, at(1_000)),
            activates(3)
        );
        // Each dock window's memory runs on its own.
        engine.note_scroll_activation(SECOND_DOCK, WindowId(3), at(3_000));
        assert_eq!(
            engine.scroll_action(A_KEY, SECOND_DOCK, Next, at(4_500)),
            activates(1)
        );
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Next, at(4_500)),
            activates(2)
        );
    }

    #[test]
    fn a_scroll_does_nothing_unless_cycling_over_an_app_with_windows() {
        use ScrollDirection::{Next, Previous};
        let settings = Settings {
            pinned: vec![pin(r"C:\p.exe")],
            ..Settings::default()
        };
        let mut engine = DockEngine::new(settings.clone());
        engine.apply(added(win(1, r"C:\a.exe", "A1")));
        engine.apply(added(win(2, r"C:\a.exe", "A2")));
        let state = engine.snapshot();
        assert!(keys(&state).contains(&"sep:1"), "{:?}", keys(&state));
        assert!(!settings.items.task_view);

        // `none`, the default: nothing anywhere, but an unknown key is still
        // an error.
        assert_eq!(settings.behavior.scroll_action, ScrollAction::None);
        for direction in [Next, Previous] {
            assert_eq!(
                engine.scroll_action(A_KEY, DOCK, direction, t0()),
                ScrollEffect::Nothing
            );
            assert_eq!(
                engine.scroll_action("nonsense", DOCK, direction, t0()),
                ScrollEffect::UnknownItem
            );
        }

        engine.apply(EngineInput::SettingsChanged(cycling(settings.clone())));
        assert_eq!(engine.scroll_action(A_KEY, DOCK, Next, t0()), activates(1));
        for direction in [Next, Previous] {
            // Fixed items and separators have no windows to cycle.
            for key in ["fixed:launcher", "fixed:recycle-bin", "sep:1"] {
                assert_eq!(
                    engine.scroll_action(key, DOCK, direction, t0()),
                    ScrollEffect::Nothing,
                    "{key}"
                );
            }
            // A pinned app without windows is not launched.
            assert_eq!(
                engine.scroll_action("app:exe:c:/p.exe", DOCK, direction, t0()),
                ScrollEffect::Nothing
            );
            // Keys of items the dock does not show.
            for key in ["nonsense", "fixed:task-view", "app:exe:c:/gone.exe"] {
                assert_eq!(
                    engine.scroll_action(key, DOCK, direction, t0()),
                    ScrollEffect::UnknownItem,
                    "{key}"
                );
            }
        }

        // A hidden app has no item: its key is unknown, as for a click.
        let mut hiding = cycling(settings);
        hiding.hidden_apps = vec![hidden(r"C:\a.exe", "A")];
        engine.apply(EngineInput::SettingsChanged(hiding));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Next, t0()),
            ScrollEffect::UnknownItem
        );
        assert_eq!(engine.primary_action(A_KEY, DOCK), ItemAction::None);

        // Its last window gone, an unpinned app's item is gone too.
        engine.apply(EngineInput::SettingsChanged(cycling(Settings::default())));
        engine.apply(removed(1));
        engine.apply(removed(2));
        assert_eq!(
            engine.scroll_action(A_KEY, DOCK, Next, t0()),
            ScrollEffect::UnknownItem
        );
    }

    #[test]
    fn a_scroll_cycles_only_the_windows_the_calling_dock_lists() {
        use ScrollDirection::{Next, Previous};
        let mut engine = two_docks(true, true);
        let mut settings = cycling(engine.settings().clone());
        engine.apply(EngineInput::SettingsChanged(settings.clone()));
        // Notepad: A (1) on the primary, B (2) and C (3) on the second
        // monitor, D (4) on the primary.
        engine.apply(added(on_monitor(win(3, NOTEPAD, "C"), SECOND)));
        engine.apply(added(on_monitor(win(4, NOTEPAD, "D"), PRIMARY)));

        // A in front: the second dock has none of its windows in front.
        engine.apply(foreground(Some(1)));
        assert_eq!(
            engine.scroll_action(NOTEPAD_KEY, DOCK, Next, t0()),
            activates(4)
        );
        assert_eq!(
            engine.scroll_action(NOTEPAD_KEY, DOCK, Previous, t0()),
            activates(4)
        );
        assert_eq!(
            engine.scroll_action(NOTEPAD_KEY, SECOND_DOCK, Next, t0()),
            activates(2)
        );
        assert_eq!(
            engine.scroll_action(NOTEPAD_KEY, SECOND_DOCK, Previous, t0()),
            activates(3)
        );
        engine.apply(foreground(Some(3)));
        assert_eq!(
            engine.scroll_action(NOTEPAD_KEY, SECOND_DOCK, Next, t0()),
            activates(2)
        );
        assert_eq!(
            engine.scroll_action(NOTEPAD_KEY, DOCK, Next, t0()),
            activates(1)
        );

        // A dock with no window of the app on its monitor: nothing, and no
        // launch of the pinned app.
        engine.apply(updated(on_monitor(win(2, NOTEPAD, "B"), PRIMARY)));
        engine.apply(updated(on_monitor(win(3, NOTEPAD, "C"), PRIMARY)));
        assert_eq!(
            engine.scroll_action(NOTEPAD_KEY, SECOND_DOCK, Next, t0()),
            ScrollEffect::Nothing
        );
        assert_eq!(
            engine.scroll_action(NOTEPAD_KEY, DOCK, Next, t0()),
            activates(4)
        );

        // The setting off: every dock cycles every window, in first-seen
        // order.
        settings.behavior.show_only_current_monitor = false;
        engine.apply(EngineInput::SettingsChanged(settings));
        for dock in [DOCK, SECOND_DOCK] {
            assert_eq!(
                engine.scroll_action(NOTEPAD_KEY, dock, Next, t0()),
                activates(4),
                "{dock}"
            );
            assert_eq!(
                engine.scroll_action(NOTEPAD_KEY, dock, Previous, t0()),
                activates(2),
                "{dock}"
            );
        }
    }
}
