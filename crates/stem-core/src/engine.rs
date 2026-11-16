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
//!   volume item is, `clock_format` only while the clock item is
//!   (`renders_differently`).
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
//!   else, and only while the clock item is shown;
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

mod accessors;
mod clicks;
mod docks;
mod platform;
mod scroll;
mod snapshot;
#[cfg(test)]
mod tests;
mod windows;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use crate::events::EngineInput;
use crate::model::{
    AppId, AppIdentity, ClockFormat, DockItemKind, DockState, MonitorId, MonitorInfo, PowerStatus,
    RecycleBinState, ThemeInfo, VolumeStatus, WindowId,
};
use crate::settings::Settings;
use docks::DockPanel;
use windows::Tracked;

pub use clicks::ItemAction;
pub use scroll::{SCROLL_MEMORY, ScrollEffect, VOLUME_STEP, volume_step};
pub use windows::app_id_for;

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
    /// on with `items.battery` on), a new volume reading while the volume
    /// item is not, and a new regional format while the clock item
    /// is not: the snapshot carries them, but no shown item draws
    /// them. (The volume and the clock item are shown whenever their setting
    /// is on, so no reading brings or takes them.)
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
}

/// Whether a page would show `after` differently from `before`: what
/// [`EngineOutcome::changed`] says (docs/ipc/events.md). Every
/// field counts but the revision and the three readings that only one item
/// shows: `power` counts while the battery item is shown, `volume` while the
/// volume item is, `clock_format` while the clock item is. Nothing
/// else on the pages reads them (the dock draws the three faces from them;
/// the settings page reads only `items`), so a new battery percent with the
/// battery item off, a slider drag with the volume item off, or a new
/// regional format with the clock off, sends nothing; the snapshot carries
/// the reading anyway, so the next state sent for another reason, and the
/// item when it is switched on, show it at once. Equal `items` show the
/// same items, so a reading is compared only when both snapshots show its
/// item, and an item that comes or goes is a change of `items` itself.
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
    if *items != after.items || *foreground != after.foreground || *docks != after.docks {
        return true;
    }
    let shows = |kind: DockItemKind| items.iter().any(|i| i.kind == kind);
    (*power != after.power && shows(DockItemKind::Battery))
        || (*volume != after.volume && shows(DockItemKind::Volume))
        || (*clock_format != after.clock_format && shows(DockItemKind::Clock))
}
